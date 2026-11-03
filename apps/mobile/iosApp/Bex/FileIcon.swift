import AgentCore
import UIKit

/// The upstream mobile catalog uses untinted Pierre PNG glyphs, including for chips.
func mobileFileImage(_ path: String) -> UIImage {
    let token = mobileFileIcon(path: path)
    let url = Bundle.main.url(forResource: "pierre_\(token)", withExtension: "png", subdirectory: "file-icons")!
    return UIImage(contentsOfFile: url.path)!
}

func mobileFileAccent(dark: Bool) -> UIColor {
    let traits = UITraitCollection(userInterfaceStyle: dark ? .dark : .light)
    let surfaces = [UIColor.systemBackground, .secondarySystemBackground, .tertiarySystemBackground].map { color in
        var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
        color.resolvedColor(with: traits).getRed(&red, green: &green, blue: &blue, alpha: &alpha)
        return UInt32((red * 255).rounded()) << 16 | UInt32((green * 255).rounded()) << 8 |
            UInt32((blue * 255).rounded())
    }
    let rgb = mobileFileChipColor(surfaces: surfaces)
    return UIColor(red: CGFloat((rgb >> 16) & 255) / 255, green: CGFloat((rgb >> 8) & 255) / 255,
                   blue: CGFloat(rgb & 255) / 255, alpha: 1)
}
