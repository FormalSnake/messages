// Renders one Apple Maps snapshot centred on a coordinate and writes it to
// stdout as PNG. Arguments: lat lon width height scale dark(0|1) span(metres).
import AppKit
import MapKit

let args = CommandLine.arguments
guard args.count == 8,
      let lat = Double(args[1]), let lon = Double(args[2]),
      let width = Double(args[3]), let height = Double(args[4]),
      let scale = Double(args[5]), let span = Double(args[7]) else {
    FileHandle.standardError.write("usage: snapshot lat lon width height scale dark span\n".data(using: .utf8)!)
    exit(64)
}
let dark = args[6] == "1"

let options = MKMapSnapshotter.Options()
let center = CLLocationCoordinate2D(latitude: lat, longitude: lon)
let aspect = width / height
options.region = MKCoordinateRegion(center: center, latitudinalMeters: aspect >= 1 ? span / aspect : span, longitudinalMeters: aspect >= 1 ? span : span * aspect)
options.size = NSSize(width: width, height: height)
options.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
options.showsBuildings = true
// Points of interest would crowd the avatar drawn over the centre.
options.pointOfInterestFilter = .excludingAll
let snapshotter = MKMapSnapshotter(options: options)

let done = DispatchSemaphore(value: 0)
var output: Data?
var failure: String?
snapshotter.start(with: DispatchQueue.global(qos: .userInitiated)) { snapshot, error in
    defer { done.signal() }
    guard let snapshot else {
        failure = error?.localizedDescription ?? "no snapshot"
        return
    }
    // The image is in points; draw it into a bitmap of exactly width*scale pixels.
    let pixelsWide = Int((width * scale).rounded()), pixelsHigh = Int((height * scale).rounded())
    guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixelsWide, pixelsHigh: pixelsHigh, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
          let context = NSGraphicsContext(bitmapImageRep: rep) else {
        failure = "could not allocate the bitmap"
        return
    }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    snapshot.image.draw(in: NSRect(x: 0, y: 0, width: pixelsWide, height: pixelsHigh))
    NSGraphicsContext.restoreGraphicsState()
    output = rep.representation(using: .png, properties: [:])
}
if done.wait(timeout: .now() + 20) == .timedOut {
    FileHandle.standardError.write("timed out\n".data(using: .utf8)!)
    exit(2)
}
guard let output else {
    FileHandle.standardError.write("\(failure ?? "no image")\n".data(using: .utf8)!)
    exit(1)
}
FileHandle.standardOutput.write(output)
