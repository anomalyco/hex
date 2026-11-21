import Foundation

/// The words \`HinglishRomanizer\` spells from a table rather than by rule: common Hindi words
/// whose casual spelling the rules miss (है hai, नहीं nahi, यह yeh), and English words
/// \`DictationTranscriber\` writes in Devanagari, back in English (मीटिंग meeting). Keys are
/// matched with and without nukta, and chandrabindu as anusvara (हूँ, हूं).
nonisolated enum HinglishLexicon {
    /// \`word\` as the tables key it: NFC, chandrabindu as anusvara, no joiners.
    static func key(_ word: String) -> String {
        var scalars = String.UnicodeScalarView()
        for scalar in word.precomposedStringWithCanonicalMapping.unicodeScalars {
            switch scalar.value {
            case 0x0901: scalars.append(Unicode.Scalar(0x0902)!)
            case 0x200C, 0x200D: continue
            default: scalars.append(scalar)
            }
        }
        return String(scalars)
    }

    static func lookup(_ key: String) -> String? {
        words[key] ?? words[withoutNukta(key)]
    }

    static func isLoanword(_ latin: String) -> Bool {
        loanwordSpellings.contains(latin)
    }

    private static func withoutNukta(_ key: String) -> String {
        String(String.UnicodeScalarView(key.unicodeScalars.filter { $0.value != 0x093C }))
    }

    static let words: [String: String] = {
        var table: [String: String] = [:]
        for (devanagari, latin) in hindi + loanwords {
            let key = key(devanagari)
            if table[key] == nil { table[key] = latin }
            let bare = withoutNukta(key)
            if table[bare] == nil { table[bare] = latin }
        }
        return table
    }()

    private static let loanwordSpellings = Set(loanwords.map(\.1))

    /// Common words the rules would spell unusually.
    private static let hindi: [(String, String)] = [
        ("है", "hai"), ("हैं", "hain"), ("नहीं", "nahi"), ("नही", "nahi"), ("मैं", "main"), ("में", "mein"),
        ("हूँ", "hoon"), ("यह", "yeh"), ("ये", "ye"), ("वह", "woh"), ("वो", "wo"), ("हम", "hum"),
        ("तुम्हें", "tumhe"), ("उन्हें", "unhe"), ("हमें", "hume"), ("इन्हें", "inhe"), ("खत्म", "khatam"),
        ("माँ", "maa"), ("छह", "chhe"), ("छः", "chhe"), ("पहले", "pehle"), ("पहली", "pehli"),
        ("रहना", "rehna"), ("कहना", "kehna"), ("बहन", "behen"), ("शहर", "shehar"), ("वाला", "wala"),
        ("वाले", "wale"), ("वाली", "wali"), ("दीदी", "didi"), ("जी", "ji"), ("क्यूँ", "kyun"),
        ("भैया", "bhaiya"), ("ठीक", "theek"), ("दिल्ली", "Delhi"), ("मुंबई", "Mumbai"),
        ("बेंगलुरु", "Bengaluru"), ("कोलकाता", "Kolkata"), ("चेन्नई", "Chennai"), ("भारत", "Bharat"),
        ("हिंदी", "Hindi"), ("काफ़ी", "kaafi"), ("अंग्रेज़ी", "English"), ("इंग्लिश", "English"), ("ॐ", "om"),
    ]

    /// English words in the Devanagari \`DictationTranscriber\` writes for them.
    private static let loanwords: [(String, String)] = [
        ("ऑफिस", "office"), ("ओफिस", "office"), ("मीटिंग", "meeting"), ("फोन", "phone"), ("कॉल", "call"),
        ("कौल", "call"), ("कोल", "call"), ("बिजी", "busy"), ("ईमेल", "email"), ("इमेल", "email"),
        ("मैसेज", "message"), ("मेसेज", "message"), ("मैसेजेस", "messages"), ("चार्ज", "charge"),
        ("चार्जर", "charger"), ("चार्जिंग", "charging"), ("वीकेंड", "weekend"), ("प्लान", "plan"),
        ("प्लानिंग", "planning"), ("टाइम", "time"), ("लेट", "late"), ("टेक्स्ट", "text"), ("डॉक्टर", "doctor"),
        ("डाक्टर", "doctor"), ("स्कूल", "school"), ("कॉलेज", "college"), ("कालेज", "college"),
        ("ट्रेन", "train"), ("बर्थडे", "birthday"), ("मूवी", "movie"), ("मिनट", "minute"), ("मिनिट", "minute"),
        ("सेकंड", "second"), ("लैपटॉप", "laptop"), ("अपडेट", "update"), ("रेडी", "ready"), ("ऑर्डर", "order"),
        ("आर्डर", "order"), ("पिज़्ज़ा", "pizza"), ("पिज्जा", "pizza"), ("कॉफ़ी", "coffee"), ("कॉफी", "coffee"),
        ("ट्रैफिक", "traffic"), ("आइडिया", "idea"), ("रिपोर्ट", "report"), ("फनी", "funny"),
        ("ट्रांसफर", "transfer"), ("ओके", "ok"), ("सॉरी", "sorry"), ("सोरी", "sorry"), ("थैंक्यू", "thank you"),
        ("थैंक्स", "thanks"), ("प्लीज", "please"), ("हेलो", "hello"), ("हैलो", "hello"), ("हाय", "hi"),
        ("बाय", "bye"), ("गुड", "good"), ("मॉर्निंग", "morning"), ("नाइट", "night"), ("इवनिंग", "evening"),
        ("बेबी", "baby"), ("फ्रेंड", "friend"), ("पार्टी", "party"), ("शॉपिंग", "shopping"), ("मार्केट", "market"),
        ("ऑनलाइन", "online"), ("ऑफलाइन", "offline"), ("इंटरनेट", "internet"), ("वाईफाई", "wifi"),
        ("नेटवर्क", "network"), ("मोबाइल", "mobile"), ("ऐप", "app"), ("एप", "app"), ("वीडियो", "video"),
        ("विडियो", "video"), ("फोटो", "photo"), ("पिक", "pic"), ("सेल्फी", "selfie"), ("कैमरा", "camera"),
        ("म्यूजिक", "music"), ("गेम", "game"), ("मैच", "match"), ("टीम", "team"), ("प्रोजेक्ट", "project"),
        ("क्लाइंट", "client"), ("मैनेजर", "manager"), ("बॉस", "boss"), ("जॉब", "job"), ("सैलरी", "salary"),
        ("बैंक", "bank"), ("कार्ड", "card"), ("पेमेंट", "payment"), ("ऑफर", "offer"), ("डिस्काउंट", "discount"),
        ("प्राइस", "price"), ("टिकट", "ticket"), ("फ्लाइट", "flight"), ("एयरपोर्ट", "airport"),
        ("स्टेशन", "station"), ("होटल", "hotel"), ("रूम", "room"), ("कार", "car"), ("बाइक", "bike"),
        ("टैक्सी", "taxi"), ("कैब", "cab"), ("ड्राइवर", "driver"), ("लोकेशन", "location"), ("एड्रेस", "address"),
        ("नंबर", "number"), ("पासवर्ड", "password"), ("ओटीपी", "OTP"), ("लिंक", "link"), ("फाइल", "file"),
        ("पीडीएफ", "PDF"), ("डॉक्यूमेंट", "document"), ("प्रेजेंटेशन", "presentation"), ("स्लाइड", "slide"),
        ("डेडलाइन", "deadline"), ("सबमिट", "submit"), ("चेक", "check"), ("कन्फर्म", "confirm"),
        ("कैंसल", "cancel"), ("बुक", "book"), ("बुकिंग", "booking"), ("रिप्लाई", "reply"), ("सेंड", "send"),
        ("फॉरवर्ड", "forward"), ("शेयर", "share"), ("डाउनलोड", "download"), ("अपलोड", "upload"),
        ("इंस्टॉल", "install"), ("प्रॉब्लम", "problem"), ("प्रोब्लम", "problem"), ("इश्यू", "issue"),
        ("सॉल्व", "solve"), ("हेल्प", "help"), ("सपोर्ट", "support"), ("टेंशन", "tension"), ("स्ट्रेस", "stress"),
        ("फ्री", "free"), ("बिजनेस", "business"), ("मार्केटिंग", "marketing"), ("सेल", "sale"),
        ("प्रोडक्ट", "product"), ("कस्टमर", "customer"), ("सर्विस", "service"), ("क्वालिटी", "quality"),
        ("ऑप्शन", "option"), ("चॉइस", "choice"), ("डिसाइड", "decide"), ("सेफ", "safe"), ("हैप्पी", "happy"),
        ("सैड", "sad"), ("बोर", "bore"), ("बोरिंग", "boring"), ("कूल", "cool"), ("नाइस", "nice"),
        ("ग्रेट", "great"), ("परफेक्ट", "perfect"), ("एक्चुअली", "actually"), ("बेसिकली", "basically"),
        ("सीरियसली", "seriously"), ("लव", "love"), ("मिस", "miss"), ("वेट", "wait"), ("स्टार्ट", "start"),
        ("स्टॉप", "stop"), ("फिनिश", "finish"), ("डन", "done"), ("लंच", "lunch"), ("डिनर", "dinner"),
        ("ब्रेकफास्ट", "breakfast"), ("केक", "cake"), ("आइसक्रीम", "ice cream"), ("चॉकलेट", "chocolate"),
        ("जिम", "gym"), ("वॉक", "walk"), ("डांस", "dance"), ("गिफ्ट", "gift"), ("सरप्राइज", "surprise"),
        ("मैडम", "madam"), ("अंकल", "uncle"), ("आंटी", "aunty"), ("मम्मी", "mummy"),
        ("डैडी", "daddy"), ("मॉम", "mom"), ("डैड", "dad"), ("हॉस्पिटल", "hospital"), ("मेडिसिन", "medicine"),
        ("टेस्ट", "test"), ("रिजल्ट", "result"), ("एग्जाम", "exam"), ("क्लास", "class"), ("टीचर", "teacher"),
        ("होमवर्क", "homework"), ("पेन", "pen"), ("टीवी", "TV"), ("न्यूज", "news"), ("शो", "show"),
        ("सीरीज", "series"), ("एपिसोड", "episode"), ("नेटफ्लिक्स", "Netflix"), ("यूट्यूब", "YouTube"),
        ("व्हाट्सएप", "WhatsApp"), ("व्हाट्सऐप", "WhatsApp"), ("वॉट्सऐप", "WhatsApp"), ("इंस्टाग्राम", "Instagram"),
        ("फेसबुक", "Facebook"), ("गूगल", "Google"), ("आईफोन", "iPhone"), ("अमेज़न", "Amazon"),
        ("सिस्टम", "system"), ("कंप्यूटर", "computer"), ("सॉफ्टवेयर", "software"), ("कोड", "code"),
        ("बग", "bug"), ("सर्वर", "server"), ("डेटा", "data"), ("रिलीज", "release"), ("वर्जन", "version"),
        ("सेटिंग", "setting"), ("बैटरी", "battery"), ("स्क्रीन", "screen"), ("कीबोर्ड", "keyboard"),
        ("लाइट", "light"), ("पावर", "power"), ("एसी", "AC"), ("फ्रिज", "fridge"), ("किचन", "kitchen"),
        ("बाथरूम", "bathroom"), ("पार्किंग", "parking"), ("रोड", "road"), ("सिग्नल", "signal"), ("ट्रिप", "trip"),
        ("हॉलिडे", "holiday"), ("वीक", "week"), ("मंथ", "month"), ("ईयर", "year"), ("संडे", "Sunday"),
        ("मंडे", "Monday"), ("ट्यूसडे", "Tuesday"), ("वेडनेसडे", "Wednesday"), ("थर्सडे", "Thursday"),
        ("फ्राइडे", "Friday"), ("सैटरडे", "Saturday"), ("जनवरी", "January"), ("फरवरी", "February"),
        ("मार्च", "March"), ("अप्रैल", "April"), ("जून", "June"), ("जुलाई", "July"), ("अगस्त", "August"),
        ("सितंबर", "September"), ("अक्टूबर", "October"), ("नवंबर", "November"), ("दिसंबर", "December"),
        ("प्लस", "plus"), ("परसेंट", "percent"), ("किलो", "kilo"), ("सिंपल", "simple"), ("फास्ट", "fast"),
        ("स्लो", "slow"), ("बेस्ट", "best"), ("लास्ट", "last"), ("नेक्स्ट", "next"), ("फर्स्ट", "first"),
        ("सेकेंड", "second"), ("कॉन्फ्रेंस", "conference"), ("इंटरव्यू", "interview"), ("ऑफिशियल", "official"),
        ("अर्जेंट", "urgent"), ("इम्पोर्टेंट", "important"), ("मेल", "mail"), ("चैट", "chat"),
        ("ग्रुप", "group"), ("लोन", "loan"), ("ईएमआई", "EMI"),
        ("यूपीआई", "UPI"), ("पेटीएम", "Paytm"), ("स्विगी", "Swiggy"), ("ज़ोमैटो", "Zomato"), ("उबर", "Uber"),
        ("ओला", "Ola"), ("डिलीवरी", "delivery"), ("पैकेट", "packet"), ("पार्सल", "parcel"), ("रिटर्न", "return"),
        ("रिफंड", "refund"), ("बिल", "bill"), ("ब्रेड", "bread"), ("बटर", "butter"), ("रिचार्ज", "recharge"),
        ("सिम", "SIM"), ("प्लेन", "plane"),
    ]

    /// Apple's \`SpeechTranscriber\` Hindi in Latin letters, where people usually spell it another way.
    static let latinSpellings: [String: String] = [
        "mei": "main", "hoo": "hoon", "nahin": "nahi", "khatm": "khatam", "jaega": "jayega",
        "jaaega": "jayega", "aaega": "aayega", "gae": "gaye",
    ]

    /// Common Hindi words in Latin letters: a capital on one mid-sentence is Apple's, not a name's.
    static let commonLatin: Set<String> = [
        "hai", "hain", "ho", "hoon", "hu", "hoo", "tha", "thi", "the", "kya", "kar", "karo", "karna",
        "karta", "karte", "karti", "karke", "kiya", "kiye", "ki", "ka", "ke", "ko", "ne", "se", "mein",
        "me", "par", "pe", "aur", "ya", "bhi", "hi", "to", "toh", "na", "nahi", "nahin", "mat", "ab",
        "abhi", "kab", "jab", "tab", "kabhi", "sab", "sabhi", "kuch", "kuchh", "koi", "kaun", "kaise",
        "kaisa", "kaisi", "kahan", "kahin", "kyon", "kyun", "kyunki", "yahan", "wahan", "yeh", "ye",
        "woh", "wo", "vo", "is", "us", "un", "in", "iska", "uska", "unka", "inka", "iski", "uski",
        "unki", "iske", "uske", "unke", "mera", "meri", "mere", "tera", "teri", "tere", "tumhara",
        "tumhari", "tumhare", "hamara", "hamari", "hamare", "apna", "apni", "apne", "aap", "aapka",
        "aapki", "aapke", "tum", "tu", "main", "mai", "mei", "hum", "mujhe", "tujhe", "tumhe", "hume",
        "unhe", "use", "ise", "ghar", "kaam", "baat", "baatein", "log", "logon", "din", "raat", "subah",
        "shaam", "kal", "aaj", "parson", "bahut", "bohot", "thoda", "thodi", "thode", "zyada", "kam",
        "accha", "acha", "achha", "acchi", "achhi", "achi", "acche", "bura", "buri", "theek", "thik",
        "sahi", "galat", "bada", "badi", "bade", "chota", "choti", "chote", "naya", "nayi", "naye",
        "purana", "jaldi", "der", "baad", "pehle", "saath", "paas", "door", "upar", "neeche", "andar",
        "bahar", "raha", "rahi", "rahe", "ja", "jaa", "jana", "jaana", "jao", "jata", "jate", "jati",
        "gaya", "gayi", "gaye", "gae", "aa", "aao", "aana", "aaya", "aayi", "aaye", "aaega", "aayega",
        "aaunga", "jaunga", "jaungi", "jaega", "jaaega", "jayega", "hoga", "hogi", "honge", "chal",
        "chalo", "chalte", "chalein", "chale", "dekh", "dekho", "dekhna", "dekhne", "dekha", "bol",
        "bolo", "bolna", "bola", "bata", "batao", "batana", "bataya", "sun", "suno", "sunna", "suna",
        "samajh", "samjha", "samjhe", "pata", "pasand", "chahiye", "chahie", "mil", "mila", "mili",
        "milenge", "milte", "milo", "lag", "lagi", "laga", "lagta", "lagti", "khana", "khane", "kha",
        "khaya", "pani", "paani", "peena", "peene", "pi", "piyo", "de", "do", "dena", "diya", "diye",
        "dijiye", "le", "lo", "lena", "liya", "liye", "bhej", "bhejo", "bhejna", "bhejdo", "sakta",
        "sakti", "sakte", "sake", "hua", "hui", "hue", "hota", "hoti", "hote", "bhai", "behen", "didi",
        "beta", "yaar", "dost", "maa", "papa", "mummy", "ji", "haan", "han", "bas", "phir", "fir",
        "ek", "teen", "char", "chaar", "panch", "paanch", "chhe", "saat", "aath", "nau", "das", "sau",
        "hazaar", "lakh", "paisa", "paise", "rupaye", "ghanta", "ghante", "mausam", "khush", "dukh",
        "pyaar", "pyar", "dil", "zindagi", "duniya", "sach", "jhooth", "matlab", "lekin", "magar",
        "agar", "isliye", "waise", "vaise", "baje", "gana", "gaana", "naam", "kamra", "khatam",
        "khatm", "karna", "padh", "padhna", "likh", "likhna", "bulaya", "bulao", "madad", "dhanyavad",
        "dhanyavaad", "shukriya", "pahunch", "pahuch", "pahunche", "intezaar", "zaroor", "zarur",
        "mushkil", "aasan", "kitna", "kitni", "kitne", "itna", "itni", "utna", "jitna", "wala", "wali",
        "wale", "bhookh", "bhook", "pyaas", "neend", "sona", "uthna", "baithna", "rukna", "ruko",
        "suniye", "dekhiye", "boliye", "kariye", "aaiye", "jaiye", "bachche", "bacche", "beti",
    ]
}
