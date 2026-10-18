// Interim main window. Phase 08 replaces it with the three-column window
// (docs/specs/mockups/window-*.html); selection plumbing is already shared.
import SwiftUI
import WhoateKit

struct MainWindowView: View {
    @Bindable var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(L10n.text(.mainWindowPending, model.language))
                .font(.system(size: 12)).foregroundStyle(.secondary)
                .padding(.horizontal, 16).padding(.vertical, 10)
            Divider()
            List(selection: $model.selectedOwnerID) {
                ForEach(model.report?.owners ?? []) { owner in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(owner.label).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                            Text(owner.verdict.text).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
                        }
                        Spacer()
                        Text(Format.bytes(owner.memBytes)).monospacedDigit()
                        Text(Format.cpu(owner.cpuPct)).monospacedDigit().foregroundStyle(.secondary)
                            .frame(width: 48, alignment: .trailing)
                    }
                    .tag(owner.id)
                }
            }
        }
        .frame(minWidth: 520, minHeight: 360)
    }
}
