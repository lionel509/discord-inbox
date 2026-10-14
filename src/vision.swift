import Foundation
import Vision

// Image bytes on stdin, recognised text on stdout, one row per line. Exit 1 when the image can't be read.
let data = FileHandle.standardInput.readDataToEndOfFile()
let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = true
// Receipts here are Traditional or Simplified Chinese with English mixed in; the default list is English only.
request.automaticallyDetectsLanguage = true
request.recognitionLanguages = ["zh-Hant", "zh-Hans", "en-US"]
do {
    try VNImageRequestHandler(data: data, options: [:]).perform([request])
} catch {
    exit(1)
}
// Receipts put the item and its price far apart on one line; join pieces whose centres share a row.
let obs = (request.results ?? []).sorted { $0.boundingBox.midY > $1.boundingBox.midY }
var rows: [[VNRecognizedTextObservation]] = []
for o in obs {
    if let first = rows.last?.first, abs(first.boundingBox.midY - o.boundingBox.midY) < first.boundingBox.height / 2 {
        rows[rows.count - 1].append(o)
    } else {
        rows.append([o])
    }
}
for row in rows {
    print(row.sorted { $0.boundingBox.minX < $1.boundingBox.minX }
        .compactMap { $0.topCandidates(1).first?.string }
        .joined(separator: "  "))
}
