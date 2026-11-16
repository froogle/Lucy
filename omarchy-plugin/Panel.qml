import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// Lucy in the bar: an icon that shows whether lucyd is stopped, waiting for
// the Wake word, in a Session or failed, and a panel to turn her on or off,
// end a Session and change her models and audio devices.
Panel {
  id: root
  moduleName: "lucy"
  ipcTarget: "lucy"

  readonly property color foreground: bar ? bar.foreground : Commons.Color.foreground
  readonly property color urgent: bar ? bar.urgent : Commons.Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.4)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  readonly property string state: lucy.state
  readonly property var snapshot: lucy.snapshot
  readonly property string glyph: state === "session" ? String.fromCodePoint(0xF05CB) : String.fromCodePoint(0xF036C)
  readonly property color iconColor: state === "failed" ? urgent
    : (state === "stopped" || state === "starting") ? Qt.darker(barForeground, 1.55)
    : barForeground
  readonly property string stateLine: state === "waiting"
    ? strings.t("state.waiting").arg(String(snapshot.wake_word))
    : strings.t("state." + state)
  readonly property string flashLine: !lucy.flashIsError ? lucy.flashText
    : lucy.flashText !== "" ? strings.t("notSavedBecause").arg(lucy.flashText) : strings.t("notSaved")
  property string openList: ""

  function label(value) {
    return value === null ? strings.t("stt.auto") : strings.t("option." + value, String(value))
  }

  function deviceLabel(choice, value) {
    var at = choice.options.indexOf(value)
    return at >= 0 && choice.labels ? choice.labels[at] : String(value)
  }

  function moreSettings() {
    lucy.openLucy()
    root.close()
  }

  readonly property var dialog: lucy.dialog
  readonly property bool asking: dialog !== null
  property string inputProblem: ""
  property bool openedToAsk: false

  onOpenedChanged: if (!opened) openList = ""
  onAskingChanged: {
    if (asking) {
      openedToAsk = !opened
      root.open()
    } else if (openedToAsk) {
      openedToAsk = false
      root.close()
    }
  }
  onDialogChanged: {
    inputProblem = ""
    if (dialog !== null) Qt.callLater(inputBox.forceActiveFocus)
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  Commons.Translations { id: strings; directory: Qt.resolvedUrl("i18n") }

  Service { id: lucy }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.glyph
    foreground: root.iconColor
    tooltipText: root.stateLine
    onPressed: function (buttonCode) { root.toggle() }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: root.asking ? inputBox : keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(flick.contentHeight, Style.space(620))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: root.asking
      onCloseRequested: root.close()
      onTabRequested: function (direction) { root.switchPanel(direction) }

      Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: root.asking ? inputBox.implicitHeight : column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        InputBox {
          id: inputBox
          x: Style.space(2)
          width: flick.width - Style.space(4)
          visible: root.asking
        }

        Column {
          id: column
          x: Style.space(2)
          width: flick.width - Style.space(4)
          spacing: Style.space(12)
          visible: !root.asking

          PanelHero {
            width: parent.width
            title: strings.t("title")
            meta: root.stateLine.toUpperCase()
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconOpacity: lucy.serviceRunning ? 1.0 : 0.5
            iconComponent: Component {
              Text {
                textFormat: Text.PlainText
                text: root.glyph
                color: root.state === "failed" ? root.urgent : root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.display
              }
            }
            trailingControl: Component {
              ToggleSwitch {
                id: powerSwitch
                objectName: "powerSwitch"
                checked: lucy.powerOn
                busy: lucy.powerBusy
                foreground: root.foreground
                onToggled: lucy.togglePower()

                PanelToolTip {
                  visible: powerSwitch.containsMouse
                  text: lucy.powerOn ? strings.t("power.off") : strings.t("power.on")
                  fontFamily: root.fontFamily
                }
              }
            }
          }

          RowLayout {
            objectName: "flash"
            visible: lucy.flashText !== "" || lucy.flashIsError || lucy.restartOffered
            width: parent.width
            spacing: Style.space(8)

            Text {
              Layout.fillWidth: true
              textFormat: Text.PlainText
              text: root.flashLine
              color: lucy.flashIsError ? root.urgent : root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.bodySmall
              wrapMode: Text.WordWrap
            }

            Button {
              objectName: "restart"
              visible: lucy.restartOffered
              text: strings.t("restart")
              fontSize: Style.font.bodySmall
              foreground: root.foreground
              fontFamily: root.fontFamily
              bordered: true
              onClicked: lucy.restart()
            }
          }

          Button {
            objectName: "endSession"
            visible: root.state === "session"
            width: parent.width
            text: strings.t("session.end")
            foreground: root.foreground
            fontFamily: root.fontFamily
            bordered: true
            onClicked: lucy.endSession()
          }

          PanelSeparator { foreground: root.foreground }

          Column {
            width: parent.width
            spacing: Style.space(6)

            Header { text: strings.t("thinking") }
            Caption { text: strings.t("thinking.fast") }
            Chips { choice: root.snapshot ? root.snapshot.fast : null }
            Caption { text: strings.t("thinking.advanced") }
            Chips { choice: root.snapshot ? root.snapshot.advanced : null }
          }

          PanelSeparator { foreground: root.foreground }

          Column {
            width: parent.width
            spacing: Style.space(6)

            Header { text: strings.t("stt") }
            Chips { choice: root.snapshot ? root.snapshot.stt : null }
          }

          PanelSeparator { foreground: root.foreground }

          Column {
            width: parent.width
            spacing: Style.space(6)

            Header { text: strings.t("audio") }
            DeviceRow {
              name: "mic"
              title: strings.t("audio.mic")
              choice: root.snapshot ? root.snapshot.mic : null
            }
            DeviceRow {
              name: "speaker"
              title: strings.t("audio.speaker")
              choice: root.snapshot ? root.snapshot.speaker : null
            }
          }

          PanelSeparator { foreground: root.foreground }

          MoreRow { width: parent.width }
        }
      }
    }
  }

  component Header: PanelSectionHeader {
    foreground: root.foreground
    fontFamily: root.fontFamily
  }

  component Caption: Text {
    textFormat: Text.PlainText
    width: parent ? parent.width : implicitWidth
    color: root.dim
    font.family: root.fontFamily
    font.pixelSize: Style.font.caption
    wrapMode: Text.WordWrap
  }

  component Chips: Row {
    id: chips
    property var choice: null
    readonly property var options: choice ? choice.options : []
    readonly property int count: Math.max(1, options.length)
    readonly property real cellWidth: (width - spacing * (count - 1)) / count
    width: parent ? parent.width : implicitWidth
    spacing: Style.space(6)

    Repeater {
      model: chips.options

      delegate: Item {
        required property var modelData
        width: chips.cellWidth
        height: chip.implicitHeight

        Button {
          id: chip
          objectName: "chip"
          width: parent.width
          text: root.label(modelData)
          tooltipText: text
          fontSize: Style.font.bodySmall
          foreground: root.foreground
          fontFamily: root.fontFamily
          horizontalPadding: Style.spacing.controlPaddingX
          verticalPadding: Style.spacing.controlPaddingY + Style.space(2)
          bordered: true
          active: JSON.stringify(lucy.current(chips.choice)) === JSON.stringify(modelData)
          onClicked: lucy.choose(chips.choice, modelData)
        }
      }
    }
  }

  component DeviceRow: Column {
    id: device
    property string name: ""
    property string title: ""
    property var choice: null
    readonly property bool listOpen: root.openList === name
    width: parent ? parent.width : implicitWidth
    spacing: Style.space(2)

    CursorSurface {
      width: parent.width
      foreground: root.foreground
      implicitHeight: deviceContent.implicitHeight + Style.spacing.rowPaddingX

      MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.openList = device.listOpen ? "" : device.name
      }

      RowLayout {
        id: deviceContent
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: Style.space(10)
        anchors.rightMargin: Style.space(10)
        spacing: Style.space(8)

        Text {
          textFormat: Text.PlainText
          text: device.title
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
        }

        Text {
          textFormat: Text.PlainText
          Layout.fillWidth: true
          horizontalAlignment: Text.AlignRight
          text: device.choice ? root.deviceLabel(device.choice, lucy.current(device.choice)) : ""
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideMiddle
        }

        Text {
          textFormat: Text.PlainText
          text: device.listOpen ? "‹" : "›"
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
        }
      }
    }

    Repeater {
      model: device.listOpen && device.choice ? device.choice.options : []

      delegate: CursorSurface {
        id: option
        required property var modelData
        readonly property bool selected: JSON.stringify(lucy.current(device.choice)) === JSON.stringify(modelData)
        width: device.width
        foreground: root.foreground
        current: selected
        implicitHeight: optionText.implicitHeight + Style.spacing.rowPaddingX

        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: {
            lucy.choose(device.choice, option.modelData)
            root.openList = ""
          }
        }

        Text {
          id: optionText
          textFormat: Text.PlainText
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          anchors.leftMargin: Style.space(24)
          anchors.rightMargin: Style.space(10)
          text: root.deviceLabel(device.choice, option.modelData)
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
          font.bold: option.selected
          elide: Text.ElideMiddle
        }
      }
    }
  }

  component MoreRow: CursorSurface {
    foreground: root.foreground
    implicitHeight: moreText.implicitHeight + Style.spacing.rowPaddingX

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: root.moreSettings()
    }

    Text {
      id: moreText
      textFormat: Text.PlainText
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      text: strings.t("more")
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
    }
  }

  // The Input dialog lucyd opened on the bar: its question, a box or a row
  // of choices per field, Send and Cancel. Esc cancels. A secret is masked,
  // and every box is emptied as the dialog goes.
  component InputBox: FocusScope {
    id: box
    implicitHeight: boxColumn.implicitHeight
    readonly property var fields: root.asking ? root.dialog.fields : []
    readonly property int firstTyped: fields.findIndex(function (field) { return field.kind !== "choice" })
    Keys.onEscapePressed: lucy.cancelInput()

    function send() {
      var values = {}
      for (var i = 0; i < entries.count; i++) {
        var entry = entries.itemAt(i)
        var problem = entry.problem()
        if (problem !== "") {
          root.inputProblem = problem
          return
        }
        values[entry.field.name] = entry.value
      }
      for (var j = 0; j < entries.count; j++) entries.itemAt(j).clear()
      lucy.answerInput(values)
    }

    Column {
      id: boxColumn
      width: parent.width
      spacing: Style.space(10)

      Header { text: strings.t("input.title") }

      Text {
        width: parent.width
        textFormat: Text.PlainText
        text: root.asking ? String(root.dialog.message) : ""
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        wrapMode: Text.WordWrap
      }

      Repeater {
        id: entries
        model: box.fields

        delegate: Column {
          id: entry
          required property var modelData
          required property int index
          readonly property var field: modelData
          readonly property bool typed: field.kind !== "choice"
          readonly property bool secret: field.kind === "secret"
          readonly property var choices: field.kind === "choice" ? field.choices : []
          property string picked: ""
          readonly property string value: !typed ? picked : secret ? typedBox.text : typedBox.text.trim()
          width: parent ? parent.width : implicitWidth
          spacing: Style.space(4)

          function clear() {
            typedBox.text = ""
            picked = ""
          }
          function problem() {
            if (value === "") return typed ? strings.t("input.empty") : strings.t("input.pick")
            if (field.kind === "number" && isNaN(Number(value))) return strings.t("input.notNumber").arg(value)
            return ""
          }

          Component.onDestruction: typedBox.text = ""

          Caption { text: entry.field.description ? String(entry.field.description) : strings.t("input." + entry.field.kind) }

          TextField {
            id: typedBox
            objectName: "inputField"
            visible: entry.typed
            focus: entry.index === box.firstTyped
            width: parent.width
            password: entry.secret
            foreground: root.foreground
            font.family: root.fontFamily
            inputMethodHints: entry.secret ? Qt.ImhSensitiveData | Qt.ImhNoPredictiveText : Qt.ImhNone
            onAccepted: box.send()
          }

          Row {
            id: choiceRow
            visible: !entry.typed
            width: parent.width
            spacing: Style.space(6)

            Repeater {
              model: entry.choices

              delegate: Button {
                required property var modelData
                objectName: "inputChoice"
                text: String(modelData)
                fontSize: Style.font.bodySmall
                foreground: root.foreground
                fontFamily: root.fontFamily
                bordered: true
                active: entry.picked === String(modelData)
                onClicked: entry.picked = String(modelData)
              }
            }
          }
        }
      }

      Text {
        visible: root.inputProblem !== ""
        width: parent.width
        textFormat: Text.PlainText
        text: root.inputProblem
        color: root.urgent
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        wrapMode: Text.WordWrap
      }

      RowLayout {
        width: parent.width
        spacing: Style.space(8)

        Button {
          objectName: "inputCancel"
          Layout.fillWidth: true
          text: strings.t("input.cancel")
          foreground: root.foreground
          fontFamily: root.fontFamily
          bordered: true
          onClicked: lucy.cancelInput()
        }

        Button {
          objectName: "inputSend"
          Layout.fillWidth: true
          text: strings.t("input.send")
          foreground: root.foreground
          fontFamily: root.fontFamily
          bordered: true
          onClicked: box.send()
        }
      }
    }
  }
}
