import SwiftUI
import UIKit

/// A photo full screen: the best version on this phone (the original over the bridge, else
/// the full thumbnail, else the LoRa preview), zoomable, with what it is and how to get a
/// better one, and Share.
struct PhotoViewer: View {
    /// An image message, or an announcement without a LoRa preview.
    let message: StoredMessage
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var error: String?

    var body: some View {
        let shown = best
        ZStack {
            Color.black.ignoresSafeArea()
            if let ui = shown?.image {
                ZoomableImage(image: ui) { dismiss() }
                    .ignoresSafeArea()
                    .accessibilityLabel("Photo")
            }
            VStack(spacing: 0) {
                HStack {
                    Button { dismiss() } label: {
                        Image(systemName: "xmark")
                            .font(.system(.body, weight: .semibold))
                            .frame(width: 44, height: 44)
                            .background(.ultraThinMaterial, in: Circle())
                    }
                    .accessibilityLabel("Close")
                    Spacer()
                    if let url = shown.flatMap(shareFile) {
                        ShareLink(item: url) {
                            Image(systemName: "square.and.arrow.up")
                                .font(.system(.body, weight: .semibold))
                                .frame(width: 44, height: 44)
                                .background(.ultraThinMaterial, in: Circle())
                        }
                        .accessibilityLabel("Share")
                    }
                }
                .foregroundStyle(.white)
                .padding(.horizontal, 16)
                Spacer()
                VStack(spacing: 10) {
                    if let shown {
                        Text(shown.caption)
                            .font(.system(.footnote))
                            .foregroundStyle(.white.opacity(0.85))
                            .multilineTextAlignment(.center)
                    }
                    actions
                    if let error {
                        Text(error).font(.system(.footnote)).foregroundStyle(Theme.sos)
                    }
                }
                .padding(.horizontal, 24)
                .padding(.bottom, 12)
            }
        }
        .preferredColorScheme(.dark)
    }

    /// What can be done for a better version: ask for the full thumbnail over LoRa, or fetch
    /// the original over the bridge.
    @ViewBuilder private var actions: some View {
        if let announce, !announce.outgoing, model.downloads[hashOf(announce)] == nil {
            if model.fetching[hashOf(announce)] != nil {
                ProgressView().tint(.white)
            } else if !model.fetchRoute(providersOf(announce)).isEmpty {
                Button("Download in full resolution") { model.fetch(announce) }
                    .buttonStyle(SecondaryButtonStyle(background: Theme.irohBg, foreground: Theme.irohFg, height: 44))
            }
        } else if case let .image(_, _, full, _, _, fullLen, _) = message.message, !full, !message.outgoing, fullLen > 0,
                  announce == nil {
            if model.requestedFull.contains(message.id) {
                Label("Full image requested…", systemImage: "hourglass")
                    .font(.system(.footnote)).foregroundStyle(.white.opacity(0.85))
            } else {
                Button("Load the full image") { error = model.requestFull(message) }
                    .buttonStyle(SecondaryButtonStyle(background: Theme.loraBg, foreground: Theme.loraFg, height: 44))
                    .disabled(!model.isConnected)
            }
        }
    }

    /// The announcement of the original, for an image or itself.
    private var announce: StoredMessage? {
        switch message.message {
        case let .image(_, mediaId, _, _, _, _, _): model.announcement(for: mediaId, in: message.chat)
        case .announce: message
        default: nil
        }
    }

    private struct Shown {
        let image: UIImage
        let caption: String
        /// The original file, shared as it is.
        let file: URL?
    }

    private var best: Shown? {
        if let announce, case let .announce(_, _, _, hash, size, providers, _) = announce.message,
           let url = model.downloads[hash], let ui = UIImage(contentsOfFile: url.path) {
            let via = providers.contains { $0.transport == 1 } ? "iroh" : "Tor"
            let bytes = ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)
            return Shown(image: ui, caption: String(localized: "Original · \(bytes) · \(via), hash verified"), file: url)
        }
        guard case let .image(_, _, full, _, _, _, data) = message.message, let ui = UIImage(data: data) else { return nil }
        let bytes = ByteCountFormatter.string(fromByteCount: Int64(data.count), countStyle: .file)
        return Shown(image: ui,
                     caption: full ? String(localized: "Full thumbnail · LoRa · \(bytes)")
                                   : String(localized: "Preview · LoRa · \(bytes)"),
                     file: nil)
    }

    /// The file to share: the original as it is, else the thumbnail as a JPEG (most apps do
    /// not open AVIF).
    private func shareFile(_ shown: Shown) -> URL? {
        if let file = shown.file { return file }
        guard let jpeg = shown.image.jpegData(compressionQuality: 0.9) else { return nil }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("lichen-photo.jpg")
        return (try? jpeg.write(to: url)) != nil ? url : nil
    }

    private func hashOf(_ m: StoredMessage) -> Data {
        if case let .announce(_, _, _, hash, _, _, _) = m.message { return hash }
        return Data()
    }

    private func providersOf(_ m: StoredMessage) -> [Provider] {
        if case let .announce(_, _, _, _, _, providers, _) = m.message { return providers }
        return []
    }
}

/// An image that zooms (pinch, double tap) and pans; dragged down when not zoomed, it closes.
private struct ZoomableImage: UIViewRepresentable {
    let image: UIImage
    let close: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(close: close) }

    func makeUIView(context: Context) -> UIScrollView {
        let scroll = UIScrollView()
        scroll.delegate = context.coordinator
        scroll.minimumZoomScale = 1
        scroll.maximumZoomScale = 6
        scroll.showsHorizontalScrollIndicator = false
        scroll.showsVerticalScrollIndicator = false
        scroll.backgroundColor = .clear
        let view = UIImageView(image: image)
        view.contentMode = .scaleAspectFit
        view.isUserInteractionEnabled = true
        view.translatesAutoresizingMaskIntoConstraints = false
        scroll.addSubview(view)
        NSLayoutConstraint.activate([
            view.widthAnchor.constraint(equalTo: scroll.frameLayoutGuide.widthAnchor),
            view.heightAnchor.constraint(equalTo: scroll.frameLayoutGuide.heightAnchor),
            view.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor),
            view.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor),
            view.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor),
            view.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor),
        ])
        context.coordinator.imageView = view
        let double = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.doubleTap(_:)))
        double.numberOfTapsRequired = 2
        view.addGestureRecognizer(double)
        let pan = UIPanGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.pan(_:)))
        pan.delegate = context.coordinator
        view.addGestureRecognizer(pan)
        return scroll
    }

    func updateUIView(_ scroll: UIScrollView, context: Context) {
        context.coordinator.imageView?.image = image
    }

    final class Coordinator: NSObject, UIScrollViewDelegate, UIGestureRecognizerDelegate {
        let close: () -> Void
        weak var imageView: UIImageView?

        init(close: @escaping () -> Void) { self.close = close }

        func viewForZooming(in scrollView: UIScrollView) -> UIView? { imageView }

        @objc func doubleTap(_ g: UITapGestureRecognizer) {
            guard let scroll = imageView?.superview as? UIScrollView else { return }
            if scroll.zoomScale > 1 {
                scroll.setZoomScale(1, animated: true)
            } else {
                let p = g.location(in: imageView)
                let size = CGSize(width: scroll.bounds.width / 2.5, height: scroll.bounds.height / 2.5)
                scroll.zoom(to: CGRect(origin: CGPoint(x: p.x - size.width / 2, y: p.y - size.height / 2), size: size),
                            animated: true)
            }
        }

        /// Not zoomed, a drag down moves the photo and closes it past a threshold.
        @objc func pan(_ g: UIPanGestureRecognizer) {
            guard let view = imageView else { return }
            let t = g.translation(in: view.superview)
            switch g.state {
            case .changed:
                view.transform = CGAffineTransform(translationX: 0, y: max(0, t.y))
            case .ended, .cancelled:
                if t.y > 120 {
                    close()
                } else {
                    UIView.animate(withDuration: 0.2) { view.transform = .identity }
                }
            default:
                break
            }
        }

        func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
            guard let pan = g as? UIPanGestureRecognizer,
                  let scroll = imageView?.superview as? UIScrollView else { return true }
            let v = pan.velocity(in: scroll)
            return scroll.zoomScale <= 1 && v.y > abs(v.x)
        }
    }
}
