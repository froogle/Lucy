import QtQuick
import Quickshell
import Quickshell.Io

// Everything the panel knows comes from `lucy status --follow --json`, and
// everything it changes goes through `lucy` or systemctl. It never reads
// config.toml or lucyd's socket itself.
Item {
  id: root

  // The latest snapshot, or null before the first and after `lucy` exits.
  property var snapshot: null
  readonly property string state: snapshot ? String(snapshot.state) : "stopped"
  readonly property bool serviceRunning: state === "starting" || state === "waiting" || state === "session"

  // What was just chosen, by key, shown until a snapshot agrees.
  property var chosen: ({})

  function apply(line) {
    var parsed
    try { parsed = JSON.parse(String(line)) } catch (e) { return }
    if (!parsed || typeof parsed !== "object") return
    snapshot = parsed
    var kept = ({})
    for (var key in chosen) {
      if (!agrees(key, chosen[key])) kept[key] = chosen[key]
    }
    chosen = kept
    _settlePower()
  }

  function agrees(key, value) {
    for (var name in snapshot) {
      var choice = snapshot[name]
      if (choice && choice.key === key) return JSON.stringify(choice.current) === JSON.stringify(value)
    }
    return true
  }

  // What `choice` is, counting a change still on its way.
  function current(choice) {
    if (!choice) return null
    return Object.prototype.hasOwnProperty.call(chosen, choice.key) ? chosen[choice.key] : choice.current
  }

  // ---- Power --------------------------------------------------------------
  // What was asked for is held until a snapshot agrees (-1: follow lucyd),
  // so the knob moves at once and a snapshot on its way can't throw it back.
  property int _desiredRunning: -1
  readonly property bool powerOn: _desiredRunning === -1 ? serviceRunning : _desiredRunning === 1
  readonly property bool powerBusy: _desiredRunning !== -1

  function setPower(on) {
    if (powerBusy || powerProcess.running) return
    _desiredRunning = on ? 1 : 0
    // A crashed lucyd hits systemd's start limit; clear it first.
    powerProcess.command = on
      ? ["sh", "-c", '"$0" --user reset-failed "$1" 2>/dev/null; exec "$0" --user start "$1"',
         "systemctl", "lucyd.service"]
      : ["systemctl", "--user", "stop", "lucyd.service"]
    powerProcess.running = true
    powerDeadline.restart()
  }
  function togglePower() { setPower(!powerOn) }

  function _settlePower() {
    if (_desiredRunning === -1 || serviceRunning !== (_desiredRunning === 1)) return
    _desiredRunning = -1
    powerDeadline.stop()
  }

  function _releasePower() {
    _desiredRunning = -1
    powerDeadline.stop()
  }

  function restart() {
    restartOffered = false
    flashText = ""
    Quickshell.execDetached(["systemctl", "--user", "restart", "lucyd.service"])
  }

  function endSession() {
    Quickshell.execDetached(["lucy", "end"])
  }

  function openLucy() {
    // TUI.float is a floating window under Omarchy's window rules.
    Quickshell.execDetached(["omarchy-launch-tui", "--app-id=TUI.float", "lucy"])
  }

  // ---- Changes ------------------------------------------------------------
  // One `lucy set` at a time, in order; a newer value for a key replaces one
  // still waiting.
  property var _queue: []

  function choose(choice, value) {
    if (!choice) return
    var next = Object.assign({}, chosen)
    next[choice.key] = value
    chosen = next
    var argv = value === null ? ["unset", choice.key] : ["set", choice.key, String(value)]
    _queue = _queue.filter(function (entry) { return entry.key !== choice.key })
      .concat([{ key: choice.key, argv: argv }])
    _next()
  }

  function _next() {
    if (changeProcess.running || _queue.length === 0) return
    var entry = _queue[0]
    _queue = _queue.slice(1)
    changeProcess.key = entry.key
    changeProcess.command = ["lucy"].concat(entry.argv)
    changeProcess.running = true
  }

  // The flash line: when a change applies, or why it wasn't saved.
  property string flashText: ""
  property bool flashIsError: false
  property bool restartOffered: false

  function flash(message, error) {
    flashText = message
    flashIsError = error === true
    flashTimer.restart()
  }

  function _written(exitCode, stdout, stderr) {
    if (exitCode !== 0) {
      var next = Object.assign({}, chosen)
      delete next[changeProcess.key]
      chosen = next
      var why = String(stderr || "").trim().split("\n").pop()
      flash(why, true)
      return
    }
    var answer
    try { answer = JSON.parse(String(stdout)) } catch (e) { return }
    // Once a change needs a restart, the offer stays until it is taken.
    restartOffered = restartOffered || answer.applies === "restart"
    flash(String(answer.message || ""), false)
  }

  Timer {
    id: flashTimer
    interval: 4000
    // A Restart button stays until it is pressed.
    onTriggered: if (!root.restartOffered) root.flashText = ""
  }

  Timer {
    id: powerDeadline
    interval: 15000
    onTriggered: root._releasePower()
  }

  Process {
    id: powerProcess
    onExited: function (exitCode) {
      if (exitCode !== 0) root._releasePower()
    }
  }

  Process {
    id: changeProcess
    property string key: ""
    stdout: StdioCollector { id: changeStdout; waitForEnd: true }
    stderr: StdioCollector { id: changeStderr; waitForEnd: true }
    onExited: function (exitCode) {
      root._written(exitCode, changeStdout.text, changeStderr.text)
      root._next()
    }
  }

  Process {
    id: follow
    command: ["lucy", "status", "--follow", "--json"]
    running: true
    stdout: SplitParser { onRead: function (line) { root.apply(line) } }
    onExited: {
      root.snapshot = null
      followAgain.restart()
    }
  }

  Timer {
    id: followAgain
    interval: 5000
    onTriggered: follow.running = true
  }
}
