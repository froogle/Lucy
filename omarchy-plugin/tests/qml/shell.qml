import Quickshell
import QtQuick
import QtTest
import qs.Commons as Commons

// Drives the real Panel.qml offscreen, against the fake lucy, systemctl and
// omarchy-launch-tui that tests/run.sh puts on PATH. Each step runs after
// the commands the one before started have finished; tests/run.sh then
// checks the commands that ran and the replies sent to `lucy input --json`.
// The panel sits in a window so TestEvent's keys reach it.
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
        mic: { key: "audio.mic", current: "default", options: ["default", "alsa_input.usb-Yeti"],
               labels: ["Default (Yeti Analog Stereo)", "Yeti Analog Stereo"] },
        speaker: { key: "audio.speaker", current: "alsa_output.gone", options: ["default", "alsa_output.hdmi", "alsa_output.gone"],
                   labels: ["Default (HDMI Digital Stereo)", "HDMI Digital Stereo", "alsa_output.gone (not connected)"] }
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

    readonly property string secret: "sk-test-not-a-real-key"

    function ask(id, message, fields) {
      lucy.inputEvent(JSON.stringify({ jsonrpc: "2.0", method: "input.open", params: { id: id, message: message, fields: fields } }))
    }

    function details(id, fields, items) {
      lucy.inputEvent(JSON.stringify({ jsonrpc: "2.0", method: "input.open",
        params: { id: id, message: "Trash these emails?", fields: fields, items: items } }))
    }

    readonly property var mail: [
      "## Ann Example\n*Rent for December*, received **Mon 23 Nov**\n\n> Could you confirm the new amount before Friday?",
      "## Billing\nYour order shipped:\n\n- 2 × `USB-C cable`\n- 1 × desk lamp\n\n<b>Raw</b> HTML, ![a parcel](https://shop.example/p.png) and [tracking](https://shop.example/t/9)",
      "### Newsletter\n```\ncurl -s https://news.example/unsubscribe\n```"
    ]

    function visibleNamed(name) {
      return findAll(barPanel, name, []).filter(function (item) { return item.visible })
    }

    function topOf(item) {
      return item.mapToItem(barPanel, 0, 0).y
    }

    function shown(text) {
      return texts(barPanel, []).some(function (t) { return t.visible && t.text === text })
    }

    function plainly(text) {
      return texts(barPanel, []).some(function (t) { return t.visible && t.text === text && t.textFormat === Text.PlainText })
    }

    function holding(item, text) {
      if (!item) return 0
      var count = typeof item.text === "string" && item.text.indexOf(text) >= 0 ? 1 : 0
      var children = item.children || []
      for (var i = 0; i < children.length; i++) count += holding(children[i], text)
      return count
    }

    readonly property var lucy: find(barPanel, "service")
    readonly property string capture: Quickshell.env("LUCY_PANEL_CAPTURE") || ""
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
          mic: { key: "audio.mic", current: "<i>mic</i>", options: ["default", "<i>mic</i>"], labels: ["Default (<i>mic</i>)", "<i>mic</i>"] }
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
        barPanel.openList = "mic"
      },
      function () {
        var shown = texts(barPanel, []).map(function (t) { return t.text })
        check("the mic row shows the default's label", shown.indexOf("Default (Yeti Analog Stereo)") >= 0, true)
        check("its list shows each device's label", shown.indexOf("Yeti Analog Stereo") >= 0, true)
        check("not its node name", shown.indexOf("alsa_input.usb-Yeti"), -1)
        check("an unplugged pin shows as not connected", shown.indexOf("alsa_output.gone (not connected)") >= 0, true)
        barPanel.openList = ""
        lucy.choose(lucy.snapshot.mic, "alsa_input.usb-Yeti")
        check("a device can change during a Session", lucy.current(lucy.snapshot.mic), "alsa_input.usb-Yeti")
        barPanel.moreSettings()
      },
      function () {
        console.log("the Input dialog: a secret")
        ask("secret-1", "Lucy needs your Linear API key", [{ name: "LINEAR_API_KEY", kind: "secret" }])
        check("a question opens the panel", barPanel.opened, true)
        check("it shows the question", shown("Lucy needs your Linear API key"), true)
        check("not the settings", shown(barPanel.stateLine.toUpperCase()), false)
        check("a secret is masked", find(barPanel, "inputField").echoMode, TextInput.Password)
      },
      function () {
        var box = find(barPanel, "inputField")
        check("its box has the keys", box.activeFocus, true)
        keys.keyClickChar("s", Qt.NoModifier, -1)
        check("what's typed reaches it", box.text, "s")
        box.text = harness.secret
        find(barPanel, "inputSend").clicked()
        check("sending clears its box", box.text, "")
        check("sending closes the dialog", lucy.dialog, null)
        check("and the panel", barPanel.opened, false)
        check("nothing in the panel holds the secret any more", holding(barPanel, harness.secret), 0)

        console.log("the Input dialog: Cancel and Esc")
        ask("cancel-1", "Which page shall I open?", [{ name: "url", kind: "url" }])
        find(barPanel, "inputCancel").clicked()
        check("Cancel closes it", lucy.dialog, null)
        ask("esc-1", "Which page shall I open?", [{ name: "url", kind: "url" }])
      },
      function () {
        keys.keyClick(Qt.Key_Escape, Qt.NoModifier, -1)
        check("Esc closes it", lucy.dialog, null)
        check("and the panel", barPanel.opened, false)

        console.log("the Input dialog: closed by lucyd")
        ask("closed-1", "Which page shall I open?", [{ name: "url", kind: "url" }])
        lucy.inputEvent(JSON.stringify({ jsonrpc: "2.0", method: "input.close", params: { id: "another" } }))
        check("another dialog's close leaves it open", lucy.dialog !== null, true)
        lucy.inputEvent(JSON.stringify({ jsonrpc: "2.0", method: "input.close", params: { id: "closed-1" } }))
        check("its own close closes it", lucy.dialog, null)
        check("and the panel", barPanel.opened, false)

        console.log("the Input dialog: over the settings the user opened")
        barPanel.open()
        ask("over-1", "Which page shall I open?", [{ name: "url", kind: "url" }])
        lucy.inputEvent(JSON.stringify({ jsonrpc: "2.0", method: "input.close", params: { id: "over-1" } }))
        check("lucyd's close leaves the panel open", barPanel.opened, true)
        barPanel.close()

        console.log("the Input dialog: text, number and choice")
        ask("kinds-1", "Tell me about the cat", [
          { name: "name", kind: "text" },
          { name: "age", kind: "number", description: "<b>Age</b> in years" },
          { name: "colour", kind: "choice", choices: ["black", "ginger"] }
        ])
        var boxes = findAll(barPanel, "inputField", [])
        check("a box for each typed field", boxes.filter(function (b) { return b.visible }).length, 2)
        check("none masked", boxes.filter(function (b) { return b.echoMode !== TextInput.Normal }).length, 0)
        check("a field's description is its label, as plain text", plainly("<b>Age</b> in years"), true)
        check("its choices", findAll(barPanel, "inputChoice", []).map(function (c) { return c.text }), ["black", "ginger"])
        boxes[0].text = "Tibbles"
        boxes[1].text = "lots"
        find(barPanel, "inputSend").clicked()
        check("a number that isn't one is refused", lucy.dialog !== null, true)
        check("and says why", shown("lots isn't a number."), true)
        boxes[1].text = "12"
        find(barPanel, "inputSend").clicked()
        check("a choice must be picked", lucy.dialog !== null, true)
        findAll(barPanel, "inputChoice", [])[1].clicked()
        find(barPanel, "inputSend").clicked()
        check("then it goes", lucy.dialog, null)
      },
      function () {
        console.log("the Details window: items above the Input dialog's boxes")
        details("details-1", [{ name: "why", kind: "text" }], mail)
        var items = visibleNamed("detailsItem")
        check("a question with items opens the panel", barPanel.opened, true)
        check("each item shows", items.length, 3)
        check("as Markdown", items.filter(function (t) { return t.textFormat === Text.MarkdownText }).length, 3)
        check("with a rule between each", visibleNamed("detailsRule").length, 2)
        check("raw HTML is escaped, so it shows as text", items[1].text.indexOf("\\<b>Raw\\</b>") >= 0, true)
        check("an image is its alt text, never fetched", items[1].text.indexOf("p.png"), -1)
        check("a link shows its URL in brackets", items[1].text.indexOf("tracking \\[https://shop.example/t/9\\]") >= 0, true)
        check("code is left as it came", items[2].text.indexOf("curl -s https://news.example/unsubscribe") >= 0, true)
      },
      function () {
        var items = visibleNamed("detailsItem")
        var box = find(barPanel, "inputField")
        check("the items sit above the box", topOf(items[2]) < topOf(box), true)
        check("the box still has the keys", box.activeFocus, true)
        if (capture !== "") stage.grabToImage(function (result) { result.saveToFile(capture) })
      },
      function () {
        find(barPanel, "inputField").text = "old mail"
        find(barPanel, "inputSend").clicked()
        check("the answer still goes", lucy.dialog, null)
        check("and the items with it", visibleNamed("detailsItem").length, 0)

        console.log("the Details window: alone, beside a spoken question")
        details("details-2", [], mail)
        check("a list alone opens the panel", barPanel.opened, true)
        check("its items show", visibleNamed("detailsItem").length, 3)
        check("with no boxes", visibleNamed("inputField").length, 0)
        check("and no Send", find(barPanel, "inputSend").visible, false)
        check("Close, not Cancel", find(barPanel, "inputCancel").text, "Close")
      },
      function () {
        keys.keyClick(Qt.Key_Escape, Qt.NoModifier, -1)
        check("Esc closes it", lucy.dialog, null)
        check("and the panel", barPanel.opened, false)
        details("details-3", [], mail)
        find(barPanel, "inputCancel").clicked()
        check("Close closes it", lucy.dialog, null)
        details("details-4", [], mail)
        lucy.inputEvent(JSON.stringify({ jsonrpc: "2.0", method: "input.close", params: { id: "details-4" } }))
        check("lucyd's close closes it", lucy.dialog, null)
        check("and the panel", barPanel.opened, false)
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

  TestEvent { id: keys }

  FloatingWindow {
    implicitWidth: 400
    implicitHeight: 700

    Rectangle {
      id: stage
      anchors.fill: parent
      color: Commons.Color.background

      BarPanel {
        id: barPanel
      }
    }
  }
}
