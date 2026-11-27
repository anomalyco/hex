import SwiftUI

/// Settings › Acknowledgements: the data Hex ships, with the notices its licences ask for.
struct AcknowledgementsView: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    ForEach(Self.shown, id: \.title) { entry in
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
    /// never-suggest list (`OffensiveWords`), and the speech models Hex downloads (Parakeet V2,
    /// and Parakeet Ultra for Settings › Dictation › Model: Multilingual); keep in step with ios/README.md.
    static let entries: [Entry] = [
        Entry(
            title: "Google Books Ngram data",
            body: """
            Spatial autocorrect's word and word-pair frequencies, German's capitalisation \
            shares and French's list of words outside its model come from the Google Books Ngram Viewer data, version 3 (2020), by Google, \
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
            title: "Leipzig Corpora Collection",
            body: """
            English words newer than the books and their spellings ("ChatGPT", "TikTok") come \
            from the Leipzig Corpora Collection's English news downloads for 2023, 2024 and \
            2025, Leipzig University and the Saxon Academy of Sciences and Humanities in \
            Leipzig (D. Goldhahn, T. Eckart and U. Quasthoff, LREC 2012; \
            wortschatz-leipzig.de/en/download), licensed under Creative Commons Attribution \
            4.0 (creativecommons.org/licenses/by/4.0). Hex keeps only word counts, how many \
            sites use each word and its spellings.
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
            title: "Parakeet V2",
            body: """
            Hex dictates English with NVIDIA's Parakeet TDT 0.6B v2 \
            (huggingface.co/nvidia/parakeet-tdt-0.6b-v2), licensed under Creative Commons \
            Attribution 4.0 International (creativecommons.org/licenses/by/4.0), downloaded as \
            FluidInference's Core ML conversion (huggingface.co/FluidInference/\
            parakeet-tdt-0.6b-v2-coreml, CC BY 4.0), which stores the encoder's weights at \
            reduced precision; Hex uses its files unchanged.
            """
        ),
        Entry(
            title: "Parakeet Ultra",
            body: """
            Settings › Dictation › Model: Multilingual downloads Parakeet Ultra by Moondream \
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

    /// The entries the app shows: Polish's once the keyboard bundles its model (`pl.hextm`).
    static var shown: [Entry] {
        entries + (keyboardBundles("pl.hextm") ? polishEntries : [])
    }

    private static func keyboardBundles(_ file: String) -> Bool {
        guard let plugins = Bundle.main.builtInPlugInsURL else { return false }
        let path = plugins.appendingPathComponent("HEXKeyboard.appex").appendingPathComponent(file).path
        return FileManager.default.fileExists(atPath: path)
    }

    /// Polish's typing model (`scripts/build-typing-model.py pl`, its inputs in
    /// `POLISH_INPUTS`); keep in step with ios/README.md.
    static let polishEntries: [Entry] = [
        Entry(
            title: "National Corpus of Polish n-grams",
            body: """
            Polish word and word-pair frequencies come from the n-grams of the National Corpus \
            of Polish's balanced subcorpus (NKJP; Institute of Computer Science, Polish Academy \
            of Sciences; zil.ipipan.waw.pl/NKJPNGrams), made available by its authors under a \
            Creative Commons Attribution licence (creativecommons.org/licenses/by/; the page \
            names no version). Hex keeps the words PoliMorf \
            lists, leaves out spellings typed without their marks, and stores the frequencies \
            rounded.
            """
        ),
        Entry(
            title: "ParlaMint",
            body: """
            Which Polish spellings are missing their marks, and which words are names, is checked \
            against the Polish part of \
            ParlaMint 5.0, the Sejm and Senate transcripts 2015-2022 (Erjavec et al., \
            Multilingual comparable corpora of parliamentary debates ParlaMint 5.0, CLARIN.SI, \
            hdl.handle.net/11356/2004), licensed under Creative Commons Attribution 4.0 \
            International (creativecommons.org/licenses/by/4.0).
            """
        ),
        Entry(
            title: "PoliMorf",
            body: """
            Polish words are checked against PoliMorf (2018-07-23; Woliński, Miłkowski, \
            Ogrodniczuk, Przepiórkowski, Szałkiewicz; hdl.handle.net/11321/577).

            Portions copyright © 2011 Marek Futrega, Piotr Gackiewicz, Mirosław Prywata, \
            Włodzimierz Macewicz, Marcin Miłkowski
            Portions copyright © 2007–2018 Zygmunt Saloni, Włodzimierz Gruszczyński, \
            Marcin Woliński, Robert Wołosz, Danuta Skowrońska
            Portions copyright © 2011–2018 Institute of Computer Science, Polish Academy of Sciences

            All rights reserved.

            Redistribution and use in source and binary forms, with or without modification, \
            are permitted provided that the following conditions are met:

            Redistributions of source code must retain the above copyright notice, this list \
            of conditions and the following disclaimer.
            Redistributions in binary form must reproduce the above copyright notice, this \
            list of conditions and the following disclaimer in the documentation and/or other \
            materials provided with the distribution.

            THIS SOFTWARE IS PROVIDED BY COPYRIGHT HOLDERS “AS IS” AND ANY EXPRESS OR IMPLIED \
            WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF \
            MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT \
            SHALL COPYRIGHT HOLDERS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, \
            INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT \
            LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR \
            PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, \
            WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR \
            OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF \
            THE POSSIBILITY OF SUCH DAMAGE.
            """
        ),
    ]
}
