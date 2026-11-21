import SwiftUI

/// Settings › Acknowledgements: the data Hex ships, with the notices its licences ask for.
struct AcknowledgementsView: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    ForEach(Self.entries, id: \.title) { entry in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(entry.title)
                                .font(.system(size: 15, weight: .medium))
                                .foregroundStyle(HexTheme.textBase)
                            Text(entry.body)
                                .font(.system(size: 13))
                                .foregroundStyle(HexTheme.textMuted)
                                .textSelection(.enabled)
                        }
                    }
                }
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(HexTheme.bgBase)
            .navigationTitle("Acknowledgements")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    struct Entry {
        let title: String
        let body: String
    }

    /// Spatial autocorrect's word lists (`scripts/build-typing-model.py`), the keyboard's
    /// never-suggest list (`OffensiveWords`), and the model Experiments › Multilingual model
    /// downloads (`ParakeetUltraModel`); keep in step with ios/README.md.
    static let entries: [Entry] = [
        Entry(
            title: "Google Books Ngram data",
            body: """
            Spatial autocorrect's word and word-pair frequencies, and German's capitalisation \
            shares, come from the Google Books Ngram Viewer data, version 3 (2020), by Google, \
            licensed under Creative Commons Attribution 3.0 (creativecommons.org/licenses/by/3.0). \
            Hex sums the counts from 1980 to 2019 and keeps only words in the lists below.
            """
        ),
        Entry(
            title: "SCOWL",
            body: """
            English words are checked against SCOWL (Spell Checker Oriented Word Lists) and \
            VarCon by Kevin Atkinson, sizes up to 70 (wordlist.aspell.net).

            Copyright 2000-2018 by Kevin Atkinson

            Permission to use, copy, modify, distribute and sell these word lists, the \
            associated scripts, the output created from the scripts, and its documentation \
            for any purpose is hereby granted without fee, provided that the above copyright \
            notice appears in all copies and that both that copyright notice and this \
            permission notice appear in supporting documentation. Kevin Atkinson makes no \
            representations about the suitability of this array for any purpose. It is \
            provided "as is" without express or implied warranty.

            SCOWL's inflections come from WordNet 1.6, Copyright 1997 by Princeton \
            University. All rights reserved. THIS SOFTWARE AND DATABASE IS PROVIDED "AS IS" \
            AND PRINCETON UNIVERSITY MAKES NO REPRESENTATIONS OR WARRANTIES, EXPRESS OR \
            IMPLIED. BY WAY OF EXAMPLE, BUT NOT LIMITATION, PRINCETON UNIVERSITY MAKES NO \
            REPRESENTATIONS OR WARRANTIES OF MERCHANTABILITY OR FITNESS FOR ANY PARTICULAR \
            PURPOSE OR THAT THE USE OF THE LICENSED SOFTWARE, DATABASE OR DOCUMENTATION WILL \
            NOT INFRINGE ANY THIRD PARTY PATENTS, COPYRIGHTS, TRADEMARKS OR OTHER RIGHTS. The \
            name of Princeton University or Princeton may not be used in advertising or \
            publicity pertaining to distribution of the software and/or database.

            Its other sources (12Dicts, Moby Words II, and Brian Kelk's UK English Wordlist) \
            are in the public domain.
            """
        ),
        Entry(
            title: "List of Dirty, Naughty, Obscene, and Otherwise Bad Words",
            body: """
            The keyboard's suggestion filter uses the List of Dirty, Naughty, Obscene, and \
            Otherwise Bad Words (LDNOOBW), © 2012–2020 Shutterstock, Inc., and contributors: \
            its English, Spanish, German and Norwegian lists, revision 5faf2ba \
            (github.com/LDNOOBW/List-of-Dirty-Naughty-Obscene-and-Otherwise-Bad-Words), \
            licensed under Creative Commons Attribution 4.0 International \
            (creativecommons.org/licenses/by/4.0). Hex lowercased and classified the entries, \
            omitted ordinary, medical and search-only terms, and added common slurs, \
            inflections and spelling variants.
            """
        ),
        Entry(
            title: "Parakeet Ultra",
            body: """
            Experiments › Multilingual model downloads Parakeet Ultra by Moondream \
            (huggingface.co/moondream/parakeet-ultra), a post-training of NVIDIA's Parakeet \
            TDT 0.6B v3 (huggingface.co/nvidia/parakeet-tdt-0.6b-v3), both licensed under \
            Creative Commons Attribution 4.0 International \
            (creativecommons.org/licenses/by/4.0). Core ML conversion by FluidInference \
            (huggingface.co/FluidInference/parakeet-ultra-coreml, revision 95eaa59), which \
            stores the encoder's weights as 8-bit integers and re-exports the decoder and \
            joint; Hex uses its files unchanged.
            """
        ),
        Entry(
            title: "RLA-ES",
            body: """
            Spanish words are checked against RLA-ES, the Spanish spelling dictionaries for \
            LibreOffice by Santiago Bosio and contributors (Spain and Mexico), used under the \
            Mozilla Public License 1.1 (github.com/sbosio/rla-es).
            """
        ),
    ]
}
