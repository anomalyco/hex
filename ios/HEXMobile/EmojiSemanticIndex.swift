import Accelerate
import Foundation
import NaturalLanguage

/// Words near in meaning to a context word, among the emoji index's own words, from the
/// word embedding Apple ships with iOS (NaturalLanguage, on device, nothing downloaded by
/// Hex). "congrats" finds "celebration" and "party", whose emoji the words alone miss.
///
/// Built when the emoji panel opens and dropped when it closes: the vocabulary's vectors
/// are the only memory it holds beyond the system's embedding.
nonisolated final class EmojiSemanticIndex: @unchecked Sendable {
    /// Below this cosine similarity a word isn't near enough to suggest anything.
    static let threshold: Float = 0.55
    static let neighborLimit = 6

    let dimension: Int
    private let embedding: NLEmbedding
    private let words: [String]
    /// `words.count` rows of `dimension` floats, each of unit length.
    private let matrix: [Float]

    var vocabularySize: Int { words.count }
    var byteCount: Int { matrix.count * MemoryLayout<Float>.size }

    /// Nil where iOS has no word embedding for the language (Norwegian).
    init?(language: String, vocabulary: [String]) {
        let nlLanguage: NLLanguage = switch language {
        case "en": .english
        case "es": .spanish
        case "de": .german
        default: NLLanguage(rawValue: language)
        }
        guard let embedding = NLEmbedding.wordEmbedding(for: nlLanguage) else { return nil }
        let dimension = embedding.dimension
        var kept: [String] = []
        var matrix: [Float] = []
        kept.reserveCapacity(vocabulary.count)
        matrix.reserveCapacity(vocabulary.count * dimension)
        // The index already leaves stopwords out.
        for word in vocabulary where word.count >= 3 {
            guard let vector = embedding.vector(for: word), vector.count == dimension else { continue }
            Self.appendUnit(vector, to: &matrix)
            kept.append(word)
        }
        guard !kept.isEmpty else { return nil }
        self.embedding = embedding
        self.dimension = dimension
        self.words = kept
        self.matrix = matrix
    }

    private static func appendUnit(_ vector: [Double], to matrix: inout [Float]) {
        var norm = 0.0
        vDSP_svesqD(vector, 1, &norm, vDSP_Length(vector.count))
        let scale = norm > 0 ? 1 / norm.squareRoot() : 0
        for value in vector { matrix.append(Float(value * scale)) }
    }

    /// The vocabulary words nearest `word`, most similar first, above `threshold`.
    func neighbors(of word: String) -> [(word: String, similarity: Float)] {
        // The embedding keeps names capitalised ("Tokyo").
        guard let vector = embedding.vector(for: word) ?? embedding.vector(for: word.capitalized),
              vector.count == dimension else { return [] }
        var query: [Float] = []
        query.reserveCapacity(dimension)
        Self.appendUnit(vector, to: &query)
        var scores = [Float](repeating: 0, count: words.count)
        matrix.withUnsafeBufferPointer { m in
            query.withUnsafeBufferPointer { q in
                scores.withUnsafeMutableBufferPointer { out in
                    vDSP_mmul(
                        m.baseAddress!, 1, q.baseAddress!, 1, out.baseAddress!, 1,
                        vDSP_Length(words.count), 1, vDSP_Length(dimension)
                    )
                }
            }
        }
        var best: [(Int, Float)] = []
        for (index, score) in scores.enumerated() where score >= Self.threshold {
            best.append((index, score))
        }
        best.sort { $0.1 > $1.1 }
        return best.prefix(Self.neighborLimit).map { (words[$0.0], $0.1) }
    }
}
