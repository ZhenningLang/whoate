// Settings window values (Phase 11): one store, read once at launch, written on
// every change. Under WHOATE_UI_AUTOMATION the app never touches
// UserDefaults.standard (the owner's real settings): a suite at an absolute
// path when WHOATE_UI_DEFAULTS gives one, else memory (research F2, F6).
import AppKit
import Foundation
import Observation

public enum AppearanceSetting: String, CaseIterable, Sendable { case system, light, dark }
public enum LanguageSetting: String, CaseIterable, Sendable { case system, zh, en }
public enum MenuBarDisplay: String, CaseIterable, Sendable { case iconAndNumber, iconOnly }

/// Refresh cadences in seconds (research F9, D3). Standard is the cadence before
/// Phase 11; power saving doubles every interval. The curve's gap limit stays
/// 1.5 × the hidden full-sample interval so hidden points remain one run.
public enum RefreshProfile: String, CaseIterable, Sendable {
    case standard, powerSaving
    public var visible: Double { self == .standard ? 2 : 4 }
    public var hidden: Double { self == .standard ? 5 : 10 }
    public var hiddenFull: Double { self == .standard ? 30 : 60 }
    public var gapLimit: Int64 { Int64(hiddenFull * 1.5) }
}

public enum SettingsKey: String, CaseIterable, Sendable {
    case appearance, language, menuBarDisplay, refreshProfile
    case notificationsEnabled = "notifications.enabled"
    case notificationsPressure = "notifications.pressure"
    case notificationsGrowing = "notifications.growing"
    case firstLaunchDone
}

/// Values are strings or booleans; anything else reads as unset.
@MainActor public protocol SettingsStore: AnyObject {
    func string(_ key: SettingsKey) -> String?
    func bool(_ key: SettingsKey) -> Bool?
    func set(_ value: String, for key: SettingsKey)
    func set(_ value: Bool, for key: SettingsKey)
}

@MainActor public final class UserDefaultsStore: SettingsStore {
    let defaults: UserDefaults
    public init(_ defaults: UserDefaults) { self.defaults = defaults }
    public func string(_ key: SettingsKey) -> String? { defaults.object(forKey: key.rawValue) as? String }
    public func bool(_ key: SettingsKey) -> Bool? { (defaults.object(forKey: key.rawValue) as? NSNumber)?.boolValue }
    public func set(_ value: String, for key: SettingsKey) { defaults.set(value, forKey: key.rawValue) }
    public func set(_ value: Bool, for key: SettingsKey) { defaults.set(value, forKey: key.rawValue) }
}

@MainActor public final class MemoryStore: SettingsStore {
    private var values: [SettingsKey: Any] = [:]
    public init() {}
    public func string(_ key: SettingsKey) -> String? { values[key] as? String }
    public func bool(_ key: SettingsKey) -> Bool? { values[key] as? Bool }
    public func set(_ value: String, for key: SettingsKey) { values[key] = value }
    public func set(_ value: Bool, for key: SettingsKey) { values[key] = value }
}

public enum StoreKind: String, Sendable { case standard, suite, memory }

public enum SettingsStores {
    /// Which store the environment asks for. Pure: the caller opens it.
    /// Automation with an unusable WHOATE_UI_DEFAULTS falls back to memory and
    /// says so on stderr (`defaults_suite_rejected`), never to the real domain.
    public static func choose(environment: [String: String]) -> (kind: StoreKind, suite: String?, rejected: Bool) {
        guard let modes = environment["WHOATE_UI_AUTOMATION"], !modes.isEmpty else { return (.standard, nil, false) }
        guard let path = environment["WHOATE_UI_DEFAULTS"], !path.isEmpty else { return (.memory, nil, false) }
        return path.hasPrefix("/") ? (.suite, path, false) : (.memory, nil, true)
    }

    @MainActor public static func make(environment: [String: String]) -> (store: any SettingsStore, kind: StoreKind) {
        let choice = choose(environment: environment)
        if choice.kind == .standard { return (UserDefaultsStore(.standard), .standard) }
        if let suite = choice.suite, let defaults = UserDefaults(suiteName: suite) { return (UserDefaultsStore(defaults), .suite) }
        if choice.rejected || choice.suite != nil { FileHandle.standardError.write(Data("defaults_suite_rejected\n".utf8)) }
        return (MemoryStore(), .memory)
    }
}

/// The settings the app reads. Every change is written to the store, then
/// `changed` runs with the key, so it takes effect at once.
@MainActor @Observable
public final class AppSettings {
    @ObservationIgnored public let store: any SettingsStore
    @ObservationIgnored public var changed: (SettingsKey) -> Void = { _ in }

    public var appearance: AppearanceSetting { didSet { if appearance != oldValue { save(appearance.rawValue, .appearance) } } }
    public var language: LanguageSetting { didSet { if language != oldValue { save(language.rawValue, .language) } } }
    public var menuBarDisplay: MenuBarDisplay {
        didSet { if menuBarDisplay != oldValue { save(menuBarDisplay.rawValue, .menuBarDisplay) } }
    }
    public var refreshProfile: RefreshProfile {
        didSet { if refreshProfile != oldValue { save(refreshProfile.rawValue, .refreshProfile) } }
    }
    /// Off by default; the two kinds only count while this is on (spec: 通知默认关).
    public var notificationsEnabled: Bool {
        didSet { if notificationsEnabled != oldValue { save(notificationsEnabled, .notificationsEnabled) } }
    }
    public var notifyPressure: Bool { didSet { if notifyPressure != oldValue { save(notifyPressure, .notificationsPressure) } } }
    public var notifyGrowing: Bool { didSet { if notifyGrowing != oldValue { save(notifyGrowing, .notificationsGrowing) } } }

    public init(store: any SettingsStore) {
        self.store = store
        appearance = store.string(.appearance).flatMap(AppearanceSetting.init(rawValue:)) ?? .system
        language = store.string(.language).flatMap(LanguageSetting.init(rawValue:)) ?? .system
        menuBarDisplay = store.string(.menuBarDisplay).flatMap(MenuBarDisplay.init(rawValue:)) ?? .iconAndNumber
        refreshProfile = store.string(.refreshProfile).flatMap(RefreshProfile.init(rawValue:)) ?? .standard
        notificationsEnabled = store.bool(.notificationsEnabled) ?? false
        notifyPressure = store.bool(.notificationsPressure) ?? true
        notifyGrowing = store.bool(.notificationsGrowing) ?? true
    }

    private func save(_ value: String, _ key: SettingsKey) { store.set(value, for: key); changed(key) }
    private func save(_ value: Bool, _ key: SettingsKey) { store.set(value, for: key); changed(key) }

    /// "跟随系统" resolves like the engine (Language.system()); the engine is only ever told zh or en.
    public func resolvedLanguage(system: () -> Language = { Language.system() }) -> Language {
        switch language {
        case .system: system()
        case .zh: .zh
        case .en: .en
        }
    }

    /// WHOATE_UI_SET="<key>=<value>,…" (QA19): through the same setters as the window.
    public func apply(_ assignments: String) {
        for item in assignments.split(separator: ",") {
            let pair = item.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            guard pair.count == 2, let key = SettingsKey(rawValue: pair[0]) else { continue }
            let value = pair[1]
            switch key {
            case .appearance: if let v = AppearanceSetting(rawValue: value) { appearance = v }
            case .language: if let v = LanguageSetting(rawValue: value) { language = v }
            case .menuBarDisplay: if let v = MenuBarDisplay(rawValue: value) { menuBarDisplay = v }
            case .refreshProfile: if let v = RefreshProfile(rawValue: value) { refreshProfile = v }
            case .notificationsEnabled: if let v = Bool(value) { notificationsEnabled = v }
            case .notificationsPressure: if let v = Bool(value) { notifyPressure = v }
            case .notificationsGrowing: if let v = Bool(value) { notifyGrowing = v }
            case .firstLaunchDone: continue
            }
        }
    }
}

public enum AppearanceApplier {
    /// The automation appearance (screenshots) wins over the setting (spec 颜色模式).
    public static func appearanceName(setting: AppearanceSetting, automation: Automation.Appearance?) -> NSAppearance.Name? {
        switch automation {
        case .light: return .aqua
        case .dark: return .darkAqua
        case nil: break
        }
        switch setting {
        case .system: return nil
        case .light: return .aqua
        case .dark: return .darkAqua
        }
    }
}
