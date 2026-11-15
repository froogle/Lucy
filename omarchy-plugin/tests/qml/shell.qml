import Quickshell
import QtQuick

// Drives the real Panel.qml offscreen, against the fake lucy, systemctl and
// omarchy-launch-tui that tests/run.sh puts on PATH. Each step runs after
// the commands the one before started have finished; tests/run.sh then
// checks the commands that ran.
ShellRoot {
  Item {
    id: harness

    property int failures: 0
    function check(name, got, want) {
      if (JSON.stringify(got) === JSON.stringify(want)) {
        console.log("  ok   " + name)
      } else {
        failures++
        console.log("  FAIL " + name + ": got " + JSON.stringify(got) + ", wanted " + JSON.stringify(want))
      }
    }

    function snapshot(state, changes) {
      var s = {
        state: state,
        wake_word: "Hey Lucy",
        fast: { key: "tiers.fast", current: "claude-fast", options: ["claude-fast", "codex-fast"] },
        advanced: { key: "tiers.advanced", current: "claude-advanced", options: ["claude-advanced", "claude-fast"] },
        stt: { key: "stt", current: null, options: [null, "small.en", "base.en", "superwhisper"] },
        mic: { key: "audio.mic", current: "default", options: ["default", "alsa_input.usb-Yeti"] },
        speaker: { key: "audio.speaker", current: "default", options: ["default", "alsa_output.hdmi"] }
      }
      for (var key in changes || {}) s[key] = changes[key]
      return s
    }

    function feed(state, changes) {
      lucy.apply(JSON.stringify(snapshot(state, changes)))
    }

    function findAll(item, name, out) {
      if (!item) return out
      if (item.objectName === name) out.push(item)
      var children = item.children || []
      for (var i = 0; i < children.length; i++) findAll(children[i], name, out)
      return out
    }

    function find(item, name) {
      return findAll(item, name, [])[0] || null
    }

    function tooltips(button) {
      var data = button.data || []
      var out = []
      for (var i = 0; i < data.length; i++) {
        if (data[i] && data[i].contentItem && data[i].delay !== undefined) out.push(data[i].contentItem)
      }
      return out
    }

    function texts(item, out) {
      if (!item) return out
      if (item.textFormat !== undefined && typeof item.text === "string") out.push(item)
      var children = item.children || []
      for (var i = 0; i < children.length; i++) texts(children[i], out)
      return out
    }

    readonly property var lucy: find(barPanel, "service")
    readonly property color dimmed: Qt.darker(barPanel.barForeground, 1.55)

    property var steps: [
      function () {
        console.log("following lucy status")
        check("the first snapshot comes from lucy status --follow --json", lucy.state, "waiting")
        check("waiting: the bar icon is a microphone", barPanel.glyph, String.fromCodePoint(0xF036C))
        check("waiting: in the bar's colour", String(barPanel.iconColor), String(barPanel.barForeground))
        check("waiting: the header says so", barPanel.stateLine, "Listening for 'Hey Lucy'")
        check("waiting: no End Session", find(barPanel, "endSession").visible, false)
        feed("waiting", { wake_word: "Ok Computer" })
        check("waiting: the header names the Wake word lucy reports", barPanel.stateLine, "Listening for 'Ok Computer'")

        feed("session")
        check("session: another glyph", barPanel.glyph, String.fromCodePoint(0xF05CB))
        check("session: the header says so", barPanel.stateLine, "In a Session")
        check("session: End Session is offered", find(barPanel, "endSession").visible, true)
        find(barPanel, "endSession").clicked()

        feed("failed")
        check("failed: red", String(barPanel.iconColor), String(barPanel.urgent))
        check("failed: the header says so", barPanel.stateLine, "lucyd failed")
        check("failed: the switch is off", lucy.powerOn, false)

        feed("stopped")
        check("stopped: dimmed", String(barPanel.iconColor), String(harness.dimmed))
        check("stopped: the header says so", barPanel.stateLine, "Stopped")

        console.log("markup from lucy is shown as text")
        feed("waiting", {
          fast: { key: "tiers.fast", current: "<b>bold</b>", options: ["<b>bold</b>"] },
          mic: { key: "audio.mic", current: "<i>mic</i>", options: ["default", "<i>mic</i>"] }
        })
        barPanel.openList = "mic"
      },
      function () {
        var shown = texts(barPanel, []).filter(function (t) { return t.text.indexOf("<") >= 0 })
        check("markup-shaped names reach the panel", shown.length >= 3, true)
        check("each is plain text", shown.filter(function (t) { return t.textFormat !== Text.PlainText }).length, 0)
        var chips = findAll(barPanel, "chip", []).filter(function (c) { return c.text === "<b>bold</b>" })
        var tips = [].concat.apply([], chips.map(tooltips))
        check("a markup-shaped Provider name reaches its chip", chips.length, 1)
        check("each chip's tooltip holds it as it came", tips.map(function (t) { return t.text }),
              chips.map(function () { return "<b>bold</b>" }))
        check("and shows it as plain text", tips.filter(function (t) { return t.textFormat !== Text.PlainText }).length, 0)
        barPanel.openList = ""

        console.log("power: a start that fails settles to the real state")
        feed("stopped")
        lucy.togglePower()
        check("the knob moves at once", lucy.powerOn, true)
        check("and waits", lucy.powerBusy, true)
      },
      function () {
        check("the refused start releases it", lucy.powerBusy, false)
        check("back to off", lucy.powerOn, false)

        console.log("power: stopping")
        feed("waiting")
        lucy.togglePower()
        check("the knob moves off at once", lucy.powerOn, false)
        feed("waiting")
        check("a stale snapshot leaves it off", lucy.powerOn, false)
      },
      function () {
        feed("stopped")
        check("the stop is confirmed", lucy.powerBusy, false)

        console.log("choices, while lucyd is stopped")
        var fast = lucy.snapshot.fast
        lucy.choose(fast, "codex-fast")
        check("the chip moves at once", lucy.current(fast), "codex-fast")
      },
      function () {
        check("the flash says when it applies", barPanel.flashLine, "Applies at the next Session.")
        check("no restart for it", lucy.restartOffered, false)
        feed("stopped", { fast: { key: "tiers.fast", current: "codex-fast", options: ["claude-fast", "codex-fast"] } })
        check("a snapshot that agrees takes over", lucy.pending, {})

        console.log("speech-to-text: Whisper's models and Superwhisper in one row")
        var labels = texts(barPanel, []).map(function (t) { return t.text })
        check("Whisper sized to this computer", labels.indexOf("Sized to this computer") >= 0, true)
        check("one of Whisper's models", labels.indexOf("base.en") >= 0, true)
        check("Superwhisper, by its name", labels.indexOf("Superwhisper") >= 0, true)
        lucy.choose(lucy.snapshot.stt, "superwhisper")
      },
      function () {
        check("Superwhisper applies at the next Session", barPanel.flashLine, "Applies at the next Session.")
        check("with no restart", lucy.restartOffered, false)
        lucy.choose(lucy.snapshot.stt, "base.en")
      },
      function () {
        check("Whisper's model applies after a restart", barPanel.flashLine, "Applies after restarting lucyd.")
        check("which is offered", find(barPanel, "restart").visible, true)
        lucy.choose(lucy.snapshot.stt, null)
      },
      function () {
        find(barPanel, "restart").clicked()
        check("Restart goes once pressed", lucy.restartOffered, false)
        lucy.choose({ key: "nope", current: "", options: [] }, "x")
      },
      function () {
        check("a refused change says it wasn't saved", barPanel.flashLine, "Not saved: lucy: nope is not a setting")
        check("and drops what was drawn", lucy.pending.hasOwnProperty("nope"), false)

        console.log("devices")
        feed("session")
        lucy.choose(lucy.snapshot.mic, "alsa_input.usb-Yeti")
        check("a device can change during a Session", lucy.current(lucy.snapshot.mic), "alsa_input.usb-Yeti")
        barPanel.moreSettings()
      },
      function () {
        console.log(failures === 0 ? "ALL OK" : failures + " FAILED")
      }
    ]
    property int step: 0

    Timer {
      interval: 600
      repeat: true
      running: harness.step < harness.steps.length
      onTriggered: harness.steps[harness.step++]()
    }
  }

  BarPanel {
    id: barPanel
  }
}
