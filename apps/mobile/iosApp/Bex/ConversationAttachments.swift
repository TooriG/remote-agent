import AgentCore
import SwiftUI
import UIKit

struct ComposerAttachmentStrip: View {
    let attachments: [Attachment]
    let media: ConversationMediaAccess
    let remove: (Int) -> Void

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 12) {
                ForEach(Array(attachments.enumerated()), id: \.element.path) { index, attachment in
                    Group {
                        if attachment.isImage {
                            ConversationImage(source: SessionImage(reference: attachment.path),
                                              label: attachment.name,
                                              identifier: "composer.attachment.\(index)",
                                              media: media,
                                              contentMode: .fill)
                                .frame(width: 120, height: 120)
                        } else {
                            Label(attachment.name, systemImage: "doc")
                                .lineLimit(1).padding(12).padding(.trailing, 24)
                        }
                    }
                    .background(Color(UIColor.secondarySystemBackground))
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .overlay(alignment: .topTrailing) {
                        Button { remove(index) } label: {
                            Image(systemName: "xmark.circle.fill")
                                .font(.title3)
                                .symbolRenderingMode(.hierarchical)
                                .foregroundStyle(.secondary)
                                .frame(width: 44, height: 44, alignment: .topTrailing)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .offset(x: 4, y: -4)
                        .accessibilityLabel("\(attachment.name)を外す")
                    }
                }
            }.padding(8)
        }
        .frame(height: attachments.contains(where: \.isImage) ? 136 : 60)
    }
}
