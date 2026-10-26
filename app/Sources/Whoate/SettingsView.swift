// Settings window (Phase 11, app/DESIGN.md §13): five tabs of grouped forms with
// system controls only, like the system's own settings windows. Every change
// goes through AppSettings, which saves it and applies it at once.
import AppKit
import SwiftUI
import WhoateKit

struct SettingsView: View {
    @Bindable var model: SettingsModel
    @Bindable var settings: AppSettings
    let app: AppModel
    /// Reports the settings window (activation policy, automation state).
    let found: (NSWindow) -> Void

    private var language: Language { app.language }

    var body: some View {
        TabView(selection: $model.tab) {
            ForEach(SettingsTab.allCases, id: \.self) { tab in
                page(tab)
                    .formStyle(.grouped)
                    .fixedSize(horizontal: false, vertical: true)
                    .tabItem { Label(L10n.text(tab.title, language), systemImage: tab.symbol) }
                    .tag(tab)
            }
        }
        .frame(width: 540)
        .background(WindowReporter { found($0) }.frame(width: 0, height: 0))
    }

    @ViewBuilder private func page(_ tab: SettingsTab) -> some View {
        switch tab {
        case .general: GeneralPage(model: model, settings: settings, language: language)
        case .menubar: MenuBarPage(settings: settings, language: language)
        case .notifications: NotificationsPage(model: model, settings: settings, language: language)
        case .cli: CommandLinePage(model: model, language: language)
        case .about: AboutPage(model: model, language: language)
        }
    }
}

private struct GeneralPage: View {
    @Bindable var model: SettingsModel
    @Bindable var settings: AppSettings
    let language: Language

    var body: some View {
        let login = model.loginPresentation(language)
        Form {
            Section {
                Toggle(L10n.text(.openAtLogin, language), isOn: Binding(get: { login.isOn }, set: { model.setLogin($0) }))
                    .disabled(!login.canToggle)
                if let message = login.message {
                    Text(message).font(.callout).foregroundStyle(.secondary)
                }
                if login.showsSystemSettingsButton {
                    Button(L10n.text(.openSystemSettings, language)) { model.login.openSystemSettings() }
                }
            }
            Section {
                Picker(L10n.text(.appearance, language), selection: $settings.appearance) {
                    Text(L10n.text(.followSystem, language)).tag(AppearanceSetting.system)
                    Text(L10n.text(.appearanceLight, language)).tag(AppearanceSetting.light)
                    Text(L10n.text(.appearanceDark, language)).tag(AppearanceSetting.dark)
                }
                Picker(L10n.text(.interfaceLanguage, language), selection: $settings.language) {
                    Text(L10n.text(.followSystem, language)).tag(LanguageSetting.system)
                    Text(L10n.text(.languageZh, language)).tag(LanguageSetting.zh)
                    Text(L10n.text(.languageEn, language)).tag(LanguageSetting.en)
                }
            }
        }
    }
}

private struct MenuBarPage: View {
    @Bindable var settings: AppSettings
    let language: Language

    var body: some View {
        let p = settings.refreshProfile
        Form {
            Section {
                Picker(L10n.text(.menuBarShows, language), selection: $settings.menuBarDisplay) {
                    Text(L10n.text(.displayIconAndNumber, language)).tag(MenuBarDisplay.iconAndNumber)
                    Text(L10n.text(.displayIconOnly, language)).tag(MenuBarDisplay.iconOnly)
                }
                .pickerStyle(.radioGroup)
            }
            Section {
                Picker(L10n.text(.backgroundRefresh, language), selection: $settings.refreshProfile) {
                    Text(L10n.text(.profileStandard, language)).tag(RefreshProfile.standard)
                    Text(L10n.text(.profilePowerSaving, language)).tag(RefreshProfile.powerSaving)
                }
                .pickerStyle(.radioGroup)
                Text(L10n.format(.profileNote, language, String(format: "%g", p.visible), String(format: "%g", p.hidden),
                                 String(format: "%g", p.hiddenFull)))
                    .font(.callout).foregroundStyle(.secondary)
            }
        }
    }
}

private struct NotificationsPage: View {
    @Bindable var model: SettingsModel
    @Bindable var settings: AppSettings
    let language: Language

    var body: some View {
        Form {
            Section {
                Toggle(L10n.text(.notifyEnabled, language),
                       isOn: Binding(get: { settings.notificationsEnabled }, set: { on in Task { await model.setNotifications(on) } }))
                if model.notificationsDenied {
                    Text(L10n.text(.notifyDenied, language)).font(.callout).foregroundStyle(.secondary)
                    Button(L10n.text(.openSystemSettings, language)) {
                        if let url = NotificationSwitch.settingsURL(bundleID: Bundle.main.bundleIdentifier) { NSWorkspace.shared.open(url) }
                    }
                }
            }
            Section {
                Toggle(L10n.text(.notifyPressureToggle, language), isOn: $settings.notifyPressure)
                Toggle(L10n.text(.notifyGrowingToggle, language), isOn: $settings.notifyGrowing)
                Text(L10n.text(.notifyNote, language)).font(.callout).foregroundStyle(.secondary)
            }
            .disabled(!settings.notificationsEnabled)
        }
    }
}

private struct CommandLinePage: View {
    @Bindable var model: SettingsModel
    let language: Language

    var body: some View {
        Form {
            Section { status }
            Section {
                Text(L10n.text(.cliAgentUsage, language)).textSelection(.enabled)
                Button(L10n.text(.cliCopyForAI, language)) {
                    Task { if let guide = await model.agentGuide() { copy(guide) } }
                }
                .disabled(!model.canCopyGuide)
            }
        }
        .task { if model.cliStatus == nil { await model.refreshCLI() } }
    }

    @ViewBuilder private var status: some View {
        switch model.cliStatus {
        case nil:
            ProgressView().controlSize(.small)
        case .installed(let path, let source, let version):
            LabeledContent(L10n.text(.cliInstalled, language)) { Image(systemName: "checkmark.circle").foregroundStyle(.secondary) }
            LabeledContent(L10n.text(.cliPathLabel, language), value: path)
            LabeledContent(L10n.text(.cliSourceLabel, language), value: L10n.text(sourceKey(source), language))
            if let version { LabeledContent(L10n.text(.cliVersionLabel, language), value: version) }
            if path.hasPrefix("/opt/homebrew/"), source == .app {
                Text(L10n.text(.cliBrewNote, language)).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
            }
        case .broken(let path):
            Text(L10n.format(.cliBroken, language, path))
            Button(L10n.text(.cliReinstall, language)) { Task { await model.installCLI() } }
        case .notInstalled:
            LabeledContent(L10n.text(.cliNotInstalled, language)) {
                Button(L10n.text(.cliInstall, language)) { Task { await model.installCLI() } }
            }
        case .noBundledCLI:
            Text(L10n.text(.cliNoBundled, language)).foregroundStyle(.secondary)
        }
        switch model.cliInstall {
        case .conflict(let path):
            Text(L10n.format(.cliConflict, language, path)).font(.callout).foregroundStyle(.secondary)
        case .manual(let command):
            Text(L10n.text(.cliManual, language)).font(.callout)
            Text(command).font(.callout.monospaced()).textSelection(.enabled)
            Text(L10n.text(.cliManualPath, language)).font(.callout).foregroundStyle(.secondary)
            Button(L10n.text(.cliCopyCommand, language)) { copy(command) }
        case .installed, nil:
            EmptyView()
        }
        if model.cliShellFailed {
            Text(L10n.text(.cliNoShell, language)).font(.callout).foregroundStyle(.secondary)
        }
    }

    private func sourceKey(_ source: CommandLineTool.Source) -> L10nKey {
        switch source {
        case .app: .cliSourceApp
        case .homebrew: .cliSourceBrew
        case .other: .cliSourceOther
        }
    }
}

private struct AboutPage: View {
    @Bindable var model: SettingsModel
    let language: Language

    var body: some View {
        let about = model.about(language)
        Form {
            Section {
                LabeledContent("whoate", value: about.isDevelopment ? about.version
                               : L10n.format(.aboutVersion, language, about.version, about.build))
                Text(about.license).foregroundStyle(.secondary)
                link(L10n.text(.aboutRepository, language), about.repository)
                link(L10n.text(.aboutReportIssue, language), about.issues)
            }
            Section {
                ForEach(about.updateLines, id: \.self) { Text($0).font(.callout).textSelection(.enabled) }
            }
            // Owner 2026-10-27: the support area stays out of sight until it has entries.
            if !about.support.isEmpty {
                Section(L10n.text(.aboutSupport, language)) {
                    ForEach(Array(about.support.enumerated()), id: \.offset) { _, item in support(item) }
                }
            }
        }
    }

    @ViewBuilder private func link(_ title: String, _ url: String) -> some View {
        if let url = URL(string: url) { Link(title, destination: url) }
    }

    @ViewBuilder private func support(_ item: SupportItem) -> some View {
        switch item.target {
        case .link(let url):
            // Spec (owner 00:31): a support link is a button, unlike the repository links.
            if let url = URL(string: url) {
                Button(item.name) { NSWorkspace.shared.open(url) }.buttonStyle(.bordered)
            }
        case .qrCode(let resource):
            if let image = SupportItem.imageURL(resource).flatMap(NSImage.init(contentsOf:)) {
                LabeledContent(item.name) {
                    Image(nsImage: image).resizable().interpolation(.none).frame(width: 64, height: 64)
                        .onTapGesture { model.enlargedSupport = resource }
                        .popover(isPresented: Binding(get: { model.enlargedSupport == resource },
                                                      set: { if !$0 { model.enlargedSupport = nil } })) {
                            Image(nsImage: image).resizable().interpolation(.none).frame(width: 280, height: 280).padding()
                        }
                }
            }
        }
    }
}
