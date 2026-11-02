import ActivityKit
import SwiftUI
import WidgetKit

@main
struct TaskWidgets: WidgetBundle {
    var body: some Widget {
        TaskActivityExtension()
    }
}

private struct TaskActivityExtension: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: TaskActivityAttributes.self) { context in
            let view = context.state.display.current
            VStack(alignment: .leading, spacing: 10) {
                Text("Bex · \(context.state.hostName)").font(.headline).lineLimit(1)
                TaskIconRow(icons: view.icons)
                    .frame(height: 22)
                Text(view.label).font(.subheadline)
            }
            .padding(16)
            .activityBackgroundTint(Color(.secondarySystemBackground))
            .activitySystemActionForegroundColor(.primary)
            .widgetURL(context.attributes.url)
        } dynamicIsland: { context in
            let view = context.state.display.current
            return DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Text("bex").font(.headline)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    Text("\(view.total)件").font(.caption)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    VStack(alignment: .leading, spacing: 8) {
                        TaskIconRow(icons: view.icons).frame(height: 22)
                        Text(view.label).font(.subheadline)
                        Text(context.state.hostName).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            } compactLeading: {
                compactIcons(view)
            } compactTrailing: {
                EmptyView()
            } minimal: {
                TaskMinimalIcons(icons: view.icons).accessibilityLabel(view.label)
            }
            .widgetURL(context.attributes.url)
        }
    }

    private func compactIcons(_ view: TaskActivityAttributes.View) -> some View {
        let icons = view.icons
        return HStack(spacing: 2) {
            TaskIconRow(icons: icons)
                .frame(width: CGFloat(icons.count) * 10, height: 22, alignment: .leading)
            if view.overflow > 0 {
                Text("+\(view.overflow)").font(.caption2).monospacedDigit()
            }
        }
    }
}

private struct TaskMinimalIcons: View {
    let icons: [TaskActivityAttributes.Icon]

    var body: some View {
        GeometryReader { geometry in
            let columns = max(1, min(3, Int(ceil(sqrt(Double(icons.count))))))
            let rows = max(1, (icons.count + columns - 1) / columns)
            let width = (geometry.size.width - CGFloat(columns - 1)) / CGFloat(columns)
            let height = (geometry.size.height - CGFloat(rows - 1)) / CGFloat(rows)
            let size = max(0, min(22, width, height))
            VStack(spacing: 1) {
                ForEach(Array(stride(from: 0, to: icons.count, by: columns)), id: \.self) { start in
                    let row = Array(icons[start ..< min(start + columns, icons.count)])
                    TaskIconRow(icons: row)
                        .frame(width: size * CGFloat(row.count) + CGFloat(row.count - 1), height: size)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(4)
    }
}

private struct TaskIconRow: View {
    let icons: [TaskActivityAttributes.Icon]

    var body: some View {
        HStack(spacing: 1) {
            ForEach(icons.indices, id: \.self) { index in
                Image(systemName: icons[index].kind.symbol)
                    .resizable().scaledToFit()
                    .foregroundStyle(icons[index].kind == .waiting ? .orange : .primary)
                    .accessibilityLabel(icons[index].label)
                    .frame(maxWidth: 22)
            }
        }
    }
}
