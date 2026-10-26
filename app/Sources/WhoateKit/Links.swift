// The one source of whoate's outside links (Phase 11): the About tab here,
// README and the product page in Phase 06. Support entries stay empty until the
// owner gives real ones (owner 2026-10-27 00:31: "先把这个空位给我留出来");
// filling them in changes `support` below and the images in app/Resources/Support,
// no code. Never a placeholder link or a sample QR code.
import Foundation

public enum Links {
    public static let repository = "https://github.com/ZhenningLang/whoate"
    public static let issues = repository + "/issues"
    /// Ways to support whoate, shown in About when not empty.
    public static let support: [SupportItem] = []
}

public struct SupportItem: Equatable, Sendable {
    public enum Target: Equatable, Sendable {
        /// A web page, opened in the browser (a button).
        case link(String)
        /// A QR code image in the app's Resources/Support folder (a small image, enlarged on click).
        case qrCode(resource: String)
    }
    /// Which interface language shows it: zh, en or all.
    public enum Audience: String, Sendable { case zh, en, all }
    public var name: String
    public var target: Target
    public var audience: Audience
    public init(name: String, target: Target, audience: Audience) {
        (self.name, self.target, self.audience) = (name, target, audience)
    }

    /// A QR code's image file: Contents/Resources/Support/<resource> (scripts/bundle-app.sh
    /// copies app/Resources/Support there). A missing file hides the item rather than
    /// showing an empty frame.
    public static func imageURL(_ resource: String, bundle: Bundle = .main) -> URL? {
        bundle.url(forResource: resource, withExtension: nil, subdirectory: "Support")
    }

    /// The items for this language: its own first, then those for all (owner 00:08).
    public static func shown(_ items: [SupportItem], language: Language) -> [SupportItem] {
        let own: Audience = language == .zh ? .zh : .en
        return items.filter { $0.audience == own } + items.filter { $0.audience == .all }
    }
}

/// The About tab's content. Pure: Info.plist and the support list are passed in.
public struct AboutInfo: Equatable, Sendable {
    public var version: String
    public var build: String
    /// No Info.plist keys (`swift run`, test wrappers): one "development build", not two.
    public var isDevelopment: Bool
    public var license: String
    public var repository: String
    public var issues: String
    /// Updates go through Homebrew; whoate never checks online (U-NG-7).
    public var updateLines: [String]
    public var support: [SupportItem]

    public static func make(info: [String: Any]?, support: [SupportItem] = Links.support, language: Language) -> Self {
        let dev = L10n.text(.aboutDevBuild, language)
        let version = (info?["CFBundleShortVersionString"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? dev
        let build = (info?["CFBundleVersion"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? dev
        return Self(version: version, build: build, isDevelopment: version == dev && build == dev,
                    license: L10n.text(.aboutLicense, language),
                    repository: Links.repository, issues: Links.issues,
                    updateLines: [L10n.text(.aboutUpdateFormula, language), L10n.text(.aboutUpdateCask, language)],
                    support: SupportItem.shown(support, language: language))
    }
}
