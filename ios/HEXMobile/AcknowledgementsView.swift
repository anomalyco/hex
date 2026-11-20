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

    /// Spatial autocorrect's word lists (`scripts/build-typing-model.py`); keep in step with
    /// ios/README.md.
    static let entries: [Entry] = [
        Entry(
            title: "Google Books Ngram data",
            body: """
            Spatial autocorrect's word and word-pair frequencies come from the Google Books \
            Ngram Viewer data, version 3 (2020), by Google, licensed under Creative Commons \
            Attribution 3.0 (creativecommons.org/licenses/by/3.0). Hex sums the counts from \
            1980 to 2019 and keeps only words in the lists below.
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
            title: "RLA-ES",
            body: """
            Spanish words are checked against RLA-ES, the Spanish spelling dictionaries for \
            LibreOffice by Santiago Bosio and contributors (Spain and Mexico), used under the \
            Mozilla Public License 1.1 (github.com/sbosio/rla-es).
            """
        ),
    ]
}
