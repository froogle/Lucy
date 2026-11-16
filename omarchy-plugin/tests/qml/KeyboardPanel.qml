import QtQuick

// Stands in for qs.Ui's KeyboardPanel in the harness: the real one is a
// layer shell window, which needs a compositor. Same properties and sizing
// helpers the panel uses, holding its content in a plain Item. Like the
// real one, it gives focusTarget the keys once it opens.
Item {
  id: root
  required property Item anchorItem
  required property QtObject bar
  property var owner: null
  property bool open: false
  property bool centerOnBar: false
  property Item focusTarget: null
  property int contentWidth: 380
  property int contentHeight: 600
  width: contentWidth
  height: contentHeight
  default property alias contentItem: holder.children

  onOpenChanged: if (open && focusTarget) Qt.callLater(function () {
    if (root.open && root.focusTarget) root.focusTarget.forceActiveFocus()
  })

  function fittedContentWidth(width, cap) {
    var desired = Math.max(1, Number(width) || 1)
    return Math.round(cap !== undefined && Number(cap) > 0 ? Math.min(desired, Number(cap)) : desired)
  }
  function fittedContentHeight(implicitHeight, cap) {
    var desired = Math.max(1, Number(implicitHeight) || 1)
    return Math.round(cap !== undefined && Number(cap) > 0 ? Math.min(desired, Number(cap)) : desired)
  }

  Item {
    id: holder
    anchors.fill: parent
  }
}
