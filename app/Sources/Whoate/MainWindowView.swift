// Interim main window. Phase 10 replaces it with the three-column window
// (docs/specs/mockups/window-*.html); selection, sort and the window/activation
// plumbing (MainWindows) are already shared and stay as they are.
import AppKit
import SwiftUI
import WhoateKit

struct MainWindowView: View {
    @Bindable var model: AppModel
    let mainWindows: MainWindows

    private var language: Language { model.language }

    var body: some View {
        let header = Panel.header(model.report?.host, sort: model.sort, language)
        VStack(alignment: .leading, spacing: 0) {
            Text(L10n.text(.mainWindowPending, language))
                .font(.system(size: 12)).foregroundStyle(.secondary)
                .padding(.horizontal, 16).padding(.top, 10)
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(header.headline).font(.system(size: 15, weight: .semibold))
                    if let detail = header.detail {
                        Text(detail).font(.system(size: 12)).foregroundStyle(.secondary).monospacedDigit()
                    }
                }
                Spacer()
                // Same model.sort as the panel: switching here switches there.
                Picker("", selection: $model.sort) {
                    Text(L10n.text(.sortMemory, language)).tag(SortKey.memory)
                    Text(L10n.text(.sortCPU, language)).tag(SortKey.cpu)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
            }
            .padding(.horizontal, 16).padding(.vertical, 10)
            Divider()
            List(selection: $model.selectedOwnerID) {
                ForEach(model.report.map { Panel.rows($0, sort: model.sort, language: language, limit: .max) } ?? []) { row in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(row.title).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                            Text(row.subtitle).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
                        }
                        Spacer()
                        Text(Format.bytes(row.memBytes)).monospacedDigit()
                        Text(Format.cpu(row.cpuPct)).monospacedDigit().foregroundStyle(.secondary)
                            .frame(width: 48, alignment: .trailing)
                    }
                    .tag(row.id)
                }
            }
        }
        .frame(minWidth: 520, minHeight: 360)
        .background(RefreshVisibility { model.setVisible(main: $0) }.frame(width: 0, height: 0))
        .background(WindowReporter { window in
            mainWindows.register(window)
            automationMainWindow(window)
        }.frame(width: 0, height: 0))
    }
}

/// Automation only. main-window mode parks the window in the main screen's
/// bottom-right corner, so the focus switch in the QA12 check (Finder) does
/// not happen to cover it; WHOATE_UI_WINDOW_FILE receives its line.
@MainActor func automationMainWindow(_ window: NSWindow) {
    let automation = WhoateApp.automation
    if automation.mainWindow, let screen = NSScreen.main?.visibleFrame {
        window.setFrameOrigin(NSPoint(x: screen.maxX - window.frame.width - 8, y: screen.minY + 8))
    }
    guard let path = automation.windowFile else { return }
    let size = window.frame.size
    appendLine("main\t\(window.windowNumber)\t\(Int(size.width))\t\(Int(size.height))\t"
        + "\(window.backingScaleFactor)\t\(ProcessInfo.processInfo.processIdentifier)\t-\n",
        to: path, failure: "window_file_failed")
}
