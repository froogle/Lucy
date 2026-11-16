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

  onOpenedChanged: if (!opened) openList = ""

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
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(620))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function (direction) { root.switchPanel(direction) }

      Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: column
          x: Style.space(2)
          width: flick.width - Style.space(4)
          spacing: Style.space(12)

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
}
