import QtQuick
import Quickshell
import Quickshell.Io

// Everything the panel knows comes from `lucy status --follow --json`, and
// everything it changes goes through `lucy`. It never reads config.toml or
// lucyd's socket itself.
Item {
  id: root
  objectName: "service"

  property var snapshot: null
  readonly property string state: snapshot ? String(snapshot.state) : "stopped"
  readonly property bool serviceRunning: state === "starting" || state === "waiting" || state === "session"

  property var pending: ({})

  function apply(line) {
    var parsed
    try { parsed = JSON.parse(String(line)) } catch (e) { return }
    if (!parsed || typeof parsed !== "object") return
    snapshot = parsed
    var stillPending = ({})
    for (var key in pending) {
      if (!agrees(key, pending[key])) stillPending[key] = pending[key]
    }
    pending = stillPending
    _settlePower()
  }

  function agrees(key, value) {
    for (var name in snapshot) {
      var choice = snapshot[name]
      if (choice && choice.key === key) return JSON.stringify(choice.current) === JSON.stringify(value)
    }
    return true
  }

  function current(choice) {
    if (!choice) return null
    return Object.prototype.hasOwnProperty.call(pending, choice.key) ? pending[choice.key] : choice.current
  }

  property bool powerBusy: false
  property bool _wantOn: false
  readonly property bool powerOn: powerBusy ? _wantOn : serviceRunning

  function setPower(on) {
    if (powerBusy || powerProcess.running) return
    _wantOn = on
    powerBusy = true
    powerProcess.command = ["lucy", "service", on ? "start" : "stop"]
    powerProcess.running = true
    powerDeadline.restart()
  }
  function togglePower() { setPower(!powerOn) }

  function _settlePower() {
    if (powerBusy && serviceRunning === _wantOn) _releasePower()
  }

  function _releasePower() {
    powerBusy = false
    powerDeadline.stop()
  }

  function restart() {
    restartOffered = false
    flashText = ""
    Quickshell.execDetached(["lucy", "service", "restart"])
  }

  function endSession() {
    Quickshell.execDetached(["lucy", "end"])
  }

  readonly property string floatingAppId: "TUI.float"

  function openLucy() {
    Quickshell.execDetached(["omarchy-launch-tui", "--app-id=" + floatingAppId, "lucy"])
  }

  property var _queue: []

  function choose(choice, value) {
    if (!choice) return
    var next = Object.assign({}, pending)
    next[choice.key] = value
    pending = next
    var argv = value === null ? ["unset", choice.key] : ["set", choice.key, String(value)]
    var others = _queue.filter(function (entry) { return entry.key !== choice.key })
    _queue = others.concat([{ key: choice.key, argv: argv }])
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
      var next = Object.assign({}, pending)
      delete next[changeProcess.key]
      pending = next
      var why = String(stderr || "").trim().split("\n").pop()
      flash(why, true)
      return
    }
    var answer
    try { answer = JSON.parse(String(stdout)) } catch (e) { return }
    restartOffered = restartOffered || answer.applies === "restart"
    flash(String(answer.message || ""), false)
  }

  Timer {
    id: flashTimer
    interval: 4000
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

  // The Input dialog lucyd has open on the bar, as `input.open` gave it, or
  // null. What is typed stays in the panel's boxes until it is sent.
  property var dialog: null

  function inputEvent(line) {
    var event
    try { event = JSON.parse(String(line)) } catch (e) { return }
    if (!event || !event.params) return
    if (event.method === "input.open") dialog = event.params
    else if (event.method === "input.close" && dialog && event.params.id === dialog.id) dialog = null
  }

  function answerInput(values) {
    if (dialog) _reply("input.answer", { id: dialog.id, values: values })
  }

  function cancelInput() {
    if (dialog) _reply("input.cancel", { id: dialog.id })
  }

  function _reply(method, params) {
    input.write(JSON.stringify({ method: method, params: params }) + "\n")
    dialog = null
  }

  Process {
    id: input
    command: ["lucy", "input", "--json"]
    running: true
    stdinEnabled: true
    stdout: SplitParser { onRead: function (line) { root.inputEvent(line) } }
    onExited: {
      root.dialog = null
      inputAgain.restart()
    }
  }

  Timer {
    id: inputAgain
    interval: 5000
    onTriggered: input.running = true
  }
}
