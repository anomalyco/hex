// Hex for iPhone on the web: /hex/iphone (what it is and how to set it up,
// and the App Store listing's support URL) and /hex/privacy (the iPhone app's
// privacy policy, its privacy policy URL), and Hex for Android: /hex/android
// (Hex Preview, the build outside Google Play: its download, how to install
// it, and how it updates). scripts/iphone.ts writes them and
// their social cards at build, in the Labs pages' look (src/labs.ts).
//
// Every claim here is checked against ios/: Hex makes no network request of
// its own except FluidAudio's English model download (ParakeetTranscriber)
// and Send feedback's one POST to Bellhop, made only when the person taps Send
// (HEXMobile/Feedback; Android's form says the same), Hex Keyboard makes none, and the Experiments that keep data are off by
// default and write only to the App Group. The Android page is checked
// against android/ the same way (the setup's labels, the 190 MB model,
// Android 12, and Hex Preview's update check in src/preview).
import { ANDROID_DOWNLOAD_PATH, ANDROID_PATH, withBase } from "./base"
import { shell } from "./labs"

export { ANDROID_PATH }
export const IPHONE_PATH = withBase("/iphone")
export const PRIVACY_PATH = withBase("/privacy")
// What each page's social card says (scripts/iphone.ts draws them). Bump CARD_VERSION
// when a card changes, so link previews that cached the old one fetch it again.
export const CARDS = {
  iphone: { title: "A keyboard you talk to", line: "Dictate right inside the keyboard, in any app. Free, private, and on your iPhone." },
  privacy: { title: "Privacy policy", line: "What you say and type stays on your iPhone. Hex sends only the feedback you choose to send." },
  android: { title: "A keyboard you talk to", line: "Dictate right inside the keyboard, in any app. Free, private, and on your phone." },
}
const CARD_VERSION = 2
export const IPHONE_CARD_PATH = withBase(`/iphone.png?v=${CARD_VERSION}`)
export const PRIVACY_CARD_PATH = withBase(`/privacy.png?v=${CARD_VERSION}`)
export const ANDROID_CARD_PATH = withBase(`/android.png?v=${CARD_VERSION}`)
// The date the privacy policy last changed in substance; change it with the policy.
export const PRIVACY_UPDATED = "November 25, 2026"
const ISSUES = "https://github.com/anomalyco/hex/issues"
// Each Hex Preview build's SHA-256 and signing certificate, beside its download.
const ANDROID_BUILDS = "https://github.com/anomalyco/hex/blob/android-preview/README.md"
const SECURITY = "https://github.com/anomalyco/hex/security/advisories/new"
const SOURCE = "https://github.com/anomalyco/hex/tree/dev/android"

const IPHONE = { crumb: { href: IPHONE_PATH, label: "iPhone" } }
const ANDROID = { crumb: { href: ANDROID_PATH, label: "Android" } }

// Long-form copy: section headings, lists, and questions, below the Labs
// pages' title and line.
const PROSE = `
.updated { margin: 14px 0 0; color: #6f7075; font-size: 14px; }
.prose { margin: 56px 0 0; }
.prose section { padding: 28px 0 0; margin: 28px 0 0; border-top: 1px solid #1d1e21; }
.prose section:first-child { margin-top: 0; }
.prose h2 { margin: 0 0 12px; font-size: 20px; font-weight: 600; letter-spacing: -0.015em; line-height: 1.3; }
.prose h3 { margin: 22px 0 6px; font-size: 16px; font-weight: 600; letter-spacing: -0.01em; }
.prose p, .prose li { color: #b4b5ba; max-width: 38em; text-wrap: pretty; }
.prose p { margin: 0 0 12px; }
.prose ul, .prose ol { margin: 0 0 12px; padding: 0 0 0 22px; }
.prose li { border: 0; margin: 0 0 10px; padding-left: 4px; }
.prose ul { list-style: disc; }
.prose ol { list-style: decimal; }
.prose li::marker { color: #6f7075; }
.prose strong { color: #f2f2ef; font-weight: 600; }
.prose a { text-decoration-color: #4a4b50; text-underline-offset: 3px; }
.prose a:hover { text-decoration-color: currentColor; }
`

export function renderIphone(origin: string): string {
  const body = `<p class="eyebrow">Hex for iPhone</p>
<h1>A keyboard you talk to</h1>
<p class="summary">Dictate right inside the keyboard, in any app. Free, private, and on your iPhone.</p>
<div class="prose">
<section>
<h2>Set up Hex</h2>
<ol>
<li>Open Hex and tap <strong>Get started</strong>.</li>
<li>Tap <strong>Allow</strong> so Hex can use the microphone.</li>
<li>Tap <strong>Open Settings</strong>, go to <strong>Keyboards</strong>, and turn on <strong>Hex Keyboard</strong> and <strong>Allow Full Access</strong>.</li>
<li>Back in Hex, try it: tap a text field, switch to Hex Keyboard with the globe key, tap <strong>Dictate</strong>, and talk.</li>
</ol>
<p>Don't see Keyboards? Go to Settings › General › Keyboard › Keyboards › Add New Keyboard and choose Hex. Then tap Hex Keyboard there and turn on Allow Full Access.</p>
</section>
<section>
<h2>Why Full Access</h2>
<p>iOS asks for Full Access for any keyboard that does more than type. Hex Keyboard needs it to record with the microphone you allowed for Hex and to share your settings and dictations with the Hex app.</p>
<p>Hex Keyboard makes no network requests, and nothing you say or type leaves your iPhone.</p>
</section>
<section>
<h2>Common questions</h2>
<h3>Does it all happen on my iPhone?</h3>
<p>Yes. Hex turns speech into text on your iPhone, with Apple's on-device speech recognition or its own English model, and deletes the audio as soon as it's done. There's no account and no cloud. See the <a href="${PRIVACY_PATH}">privacy policy</a>.</p>
<h3>Is Hex free?</h3>
<p>Yes. No subscription, no ads, and nothing to buy in the app.</p>
<h3>Which languages does Hex speak?</h3>
<p>On iOS 26 or later, every language Apple can transcribe on the iPhone. Hex picks your iPhone's language, and you can switch from the keyboard. Before iOS 26, Hex dictates in English.</p>
<h3>Does my music keep playing?</h3>
<p>Yes. With <strong>Always use iPhone microphone</strong> on (the default), Hex records with the iPhone's own microphone, so your music keeps playing.</p>
<h3>Does Hex work offline?</h3>
<p>Yes, once the speech model for your language has downloaded. That happens once, the first time you use a language.</p>
<h3>Why does the orange microphone dot stay on?</h3>
<p>After a dictation that goes through the Hex app, Hex keeps the microphone ready so the next one starts at once. Choose how long in Hex under <strong>Keep mic warm</strong>, or turn it off.</p>
</section>
<section>
<h2>Get help</h2>
<p>Something not working, or have an idea? <a href="${ISSUES}">Open an issue on GitHub</a>. Report a security problem privately through <a href="${SECURITY}">GitHub's vulnerability reporting</a>.</p>
</section>
</div>
<section class="about">
<p>Hex also runs on Mac, Windows, and Linux.</p>
<p><a href="${withBase("/")}">Hex for your computer</a> · <a href="${PRIVACY_PATH}">Privacy policy</a></p>
</section>`
  return shell(
    origin,
    {
      title: "Hex for iPhone",
      description: "A keyboard you talk to. Dictate right inside the keyboard, in any app: free, private, and on your iPhone. Setup, questions, and help.",
      url: IPHONE_PATH,
      image: IPHONE_CARD_PATH,
      imageAlt: "Hex for iPhone: a keyboard you talk to.",
    },
    body,
    { ...IPHONE, style: PROSE },
  )
}

export function renderAndroid(origin: string): string {
  const address = `${origin.replace("https://", "")}${ANDROID_PATH.slice("/hex".length)}`
  const body = `<p class="eyebrow">Hex for Android</p>
<h1>A keyboard you talk to</h1>
<p class="summary">Dictate right inside the keyboard, in any app. Free, private, and on your phone.</p>
<div class="actions"><a class="button button--primary" href="${ANDROID_DOWNLOAD_PATH}">Download Hex Preview</a></div>
<p class="note">An APK for Android 12 or later, about 70 MB. Hex Preview is Hex for Android before it reaches Google Play. It installs as its own app and tells you when there's an update.</p>
<p class="note">Google Play Protect flagged earlier builds of Hex Preview as harmful. The current build fixes that. <a href="#play-protect">What happened</a>.</p>
<p class="note">On a computer? Open ${address} on your Android phone.</p>
<div class="prose">
<section>
<h2>Install Hex Preview</h2>
<ol>
<li>On your phone, tap <strong>Download Hex Preview</strong> above and open the file when it has downloaded.</li>
<li>Android asks whether your browser or Files app may install apps. Tap <strong>Settings</strong>, turn on <strong>Allow from this source</strong>, then go back and tap <strong>Install</strong>.</li>
<li>If Google Play Protect says <strong>App scan recommended</strong>, tap <strong>Scan app</strong>, then <strong>Install</strong> when it says the app looks safe.</li>
<li>Open Hex Preview. Its setup is one page with one button that always names the next step.</li>
<li>Tap <strong>Allow microphone</strong> and allow it. Hex starts downloading its speech model (190 MB) on Wi-Fi. On mobile data it waits for Wi-Fi, or tap <strong>Use mobile data</strong>.</li>
<li>Tap <strong>Turn on keyboard</strong>, turn on <strong>Hex Preview</strong> in Android's list, tap <strong>OK</strong> on Android's warnings, then go back. Android then shows its keyboards: under <strong>Hex Preview</strong>, choose <strong>Full keyboard</strong>.</li>
<li>Hex Preview opens a field to try it in: hold the Hex key, say a few words, then let go.</li>
</ol>
</section>
<section>
<h2>Updates</h2>
<p>Hex Preview looks for a newer build when you open it and at most once a day from the keyboard, and shows what changed. Tap <strong>Download update</strong> and your browser downloads the new build from GitHub. Open the download and tap <strong>Update</strong> when Android asks. The first time, Android asks you to allow your browser to install apps: allow it, go back, and tap the download again.</p>
<p>On a build before 2348, the button says <strong>Update</strong> and Hex Preview installs that one update itself. If Google Play Protect asks to scan it, tap <strong>Scan app</strong>, then <strong>Install</strong>.</p>
<p>Updating closes the keyboard for a moment. Tap a text field to bring it back.</p>
</section>
<section>
<h2>Common questions</h2>
<h3>Does it all happen on my phone?</h3>
<p>Yes. Hex turns speech into text on your phone and never saves a recording. There's no account and no cloud. See the <a href="${PRIVACY_PATH}#android">privacy policy</a>.</p>
<h3>Is Hex free?</h3>
<p>Yes. No subscription, no ads, and nothing to buy in the app.</p>
<h3>Why does Android warn me before installing?</h3>
<p>Android asks before installing any app that doesn't come from an app store. Anomaly builds and signs Hex Preview, and each build's SHA-256 is <a href="${ANDROID_BUILDS}">listed on GitHub</a> if you want to check a download.</p>
<h3 id="play-protect">Why did Google Play Protect say Hex Preview was harmful?</h3>
<p>Builds before 2348 could install their own updates. Google Play Protect flags any app from outside Google Play that asks for permission to install apps, so it blocked those builds, or showed <strong>Harmful app found</strong> once they were installed. Nothing in them was harmful, and <a href="${SOURCE}">the code is public</a>.</p>
<p>From build 2348, Hex Preview no longer asks to install apps, and Play Protect no longer flags it. If you have an older build, update it from Hex Preview's Home screen.</p>
<h3>What does Hex Preview ask Android for?</h3>
<p>The microphone, for dictation; the internet, for the speech model, update checks, and feedback you send; and the network state, which tells Hex whether the phone is on Wi-Fi or on mobile data, so the 190 MB speech model can wait for Wi-Fi. Android leaves the network's name out, and the network state doesn't let Hex see what goes over the network. Android asks you only about the microphone.</p>
<h3>Can I keep Hex from Google Play too?</h3>
<p>Yes. Hex Preview is its own app, with its own keyboard, settings, and speech model, so the two sit side by side.</p>
<h3>What does Hex Preview send when it looks for an update?</h3>
<p>It asks GitHub for the latest version. That request carries nothing about you or what you type, though GitHub sees your internet address, as with any website. Turn it off in Hex Preview's settings with <strong>Check for updates automatically</strong>.</p>
<h3>Does Hex work offline?</h3>
<p>Yes. Once the speech model has downloaded, dictation and typing need no connection.</p>
</section>
<section>
<h2>Get help</h2>
<p>Something not working, or have an idea? <a href="${ISSUES}">Open an issue on GitHub</a>. Report a security problem privately through <a href="${SECURITY}">GitHub's vulnerability reporting</a>.</p>
</section>
</div>
<section class="about">
<p>Hex also runs on iPhone, Mac, Windows, and Linux.</p>
<p><a href="${IPHONE_PATH}">Hex for iPhone</a> · <a href="${withBase("/")}">Hex for your computer</a> · <a href="${PRIVACY_PATH}">Privacy policy</a></p>
</section>`
  return shell(
    origin,
    {
      title: "Hex for Android",
      description: "A keyboard you talk to. Dictate right inside the keyboard, in any app: free, private, and on your phone. Download Hex Preview, set it up, and get help.",
      url: ANDROID_PATH,
      image: ANDROID_CARD_PATH,
      imageAlt: "Hex for Android: a keyboard you talk to.",
    },
    body,
    { ...ANDROID, style: PROSE },
  )
}

export function renderPrivacy(origin: string): string {
  const body = `<p class="eyebrow">Hex for iPhone and Android</p>
<h1>Privacy policy</h1>
<p class="summary">What you say and type stays on your iPhone. Hex sends only the feedback you choose to send.</p>
<p class="updated">Last updated ${PRIVACY_UPDATED}</p>
<div class="prose">
<section>
<h2>The short version</h2>
<p>Hex for iPhone and Android is made by Anomaly Innovations, Inc. This policy covers the Hex app for iPhone, Hex Keyboard, and Hex's widgets and Live Activities, and <a href="#android">Hex for Android</a> and its keyboard.</p>
<p>There's no account, no analytics, no advertising, and no tracking. Nothing you dictate or type is ever sent anywhere. The only thing Hex sends about you is feedback, and only when you write it and tap Send.</p>
</section>
<section>
<h2>What happens on your iPhone</h2>
<ul>
<li><strong>Dictation.</strong> While you dictate, Hex records audio to a temporary file on your iPhone, turns it into text on the iPhone, and deletes the file as soon as it's done or you cancel. The text goes into the field you're typing in.</li>
<li><strong>Settings.</strong> Your choices, like style, language, and keyboard options, are stored on your iPhone and shared between the Hex app and Hex Keyboard.</li>
<li><strong>Full Access.</strong> Hex Keyboard asks for Full Access so it can record with the microphone you allowed for Hex and share settings and dictations with the Hex app. Hex Keyboard makes no network requests.</li>
<li><strong>Clean up and translation.</strong> Clean up what I say, Clean up with Apple Intelligence, and Translate what I say run on your iPhone, with Hex's own model or Apple's on-device models.</li>
</ul>
</section>
<section>
<h2>Features you turn on</h2>
<p>These are off until you turn them on in Hex's settings. What they keep stays on your iPhone.</p>
<ul>
<li><strong>Custom vocabulary</strong> keeps the names and terms you add.</li>
<li><strong>Learn how I type</strong> keeps where you tap on each key and the words you corrected, with what you changed them to. It never keeps what you write. Delete what Hex learned erases it.</li>
<li><strong>Record typing logs</strong> keeps a detailed log of your typing in Hex Keyboard, including the text around your cursor, for 14 days, for testing the keyboard. It never logs password, one-time code, or card fields. The logs leave your iPhone only if you share them yourself, and you can delete them at any time.</li>
</ul>
</section>
<section id="feedback">
<h2>Feedback you send</h2>
<p>Hex on iPhone and on Android has a Send feedback form. Nothing is sent until you tap Send. Then Hex sends, to Anomaly's feedback inbox (Bellhop, run by Anomaly):</p>
<ul>
<li>What you wrote: your message, and a subject and kind if you chose them.</li>
<li>The pictures shown in the form: a picture of Hex's own screen taken when you opened the form (never the rest of your phone's screen), and any photos you add. You can remove each one before sending.</li>
<li>Hex's version, your phone's model, its system version and language, which screen of Hex you opened the form from, and the language and speech model Hex dictates with.</li>
<li>Your email, only if you type one in, so Anomaly can reply. Nothing is emailed when you send: if Anomaly replies, you'll get an email and confirm your address once. With an email, Hex also sends a random install ID, made the first time you send feedback with one, so your reports can be kept together. It isn't tied to your phone or your Apple or Google account, and deleting Hex deletes it.</li>
</ul>
<p>Without an email, your feedback is anonymous: Hex sends no email and no install ID, so it can't be answered. Feedback never includes your dictations, what you type, your vocabulary, what Hex learned about your typing, typing logs, or recordings, and Hex Keyboard never sends it. Anomaly uses feedback only to answer you and fix what you report. Like any request, the server sees your IP address.</p>
</section>
<section>
<h2>Downloads</h2>
<p>Hex downloads a speech model the first time you use a language: Apple's, through iOS, or, where Apple's isn't available (such as before iOS 26), Hex's own English model from Hugging Face. Apple Intelligence and translation languages also download through iOS.</p>
<p>These downloads never include your audio or text. Like any download, the server that sends it sees your IP address.</p>
</section>
<section>
<h2>Crash reports</h2>
<p>If you share analytics with app developers in iOS, Apple may share crash reports about Hex with us. Turn this off in Settings › Privacy &amp; Security › Analytics &amp; Improvements.</p>
</section>
<section>
<h2>Children</h2>
<p>Hex is suitable for all ages and collects no data from anyone, including children, except feedback someone chooses to send.</p>
</section>
<section id="android">
<h2>Hex for Android</h2>
<p>Hex for Android and its keyboard work the same way: no account, no analytics, no advertising, and no tracking.</p>
<ul>
<li><strong>Typing.</strong> Hex Keyboard uses the text around your cursor and where you tap to suggest and correct words on your phone, and keeps no log of what you type. In password and PIN fields it types exactly what you press, without reading the field, suggesting words, or dictating.</li>
<li><strong>Dictation.</strong> Hex uses the microphone only while you dictate. It turns speech into text in memory on your phone, never saves a recording, and keeps no history of what you said. Clean up and translation also run on your phone.</li>
<li><strong>Speech models.</strong> During setup, Hex downloads one public speech model (190 MB) from Hugging Face over HTTPS, and a second one (135 MB) only if you turn on Faster English dictation in Labs and tap Download. Neither request carries your words, audio, or device identifiers; like any download, the server sees your IP address. If the first fails, Hex uses the smaller model it comes with. The first waits for Wi-Fi unless you tap Use mobile data: to know which kind of network the phone is on, Hex has Android's network state permission, which it uses only to ask whether the network charges for data. Android leaves the network's name out, and the permission doesn't let Hex see what goes over the network.</li>
<li><strong>What Hex keeps.</strong> Your settings, setup progress, and the speech models you downloaded, in Hex's private storage; Delete download in Labs removes the second one. When you undo an autocorrection, Hex keeps a count against a hash of that word pair, never the words, so it doesn't make the same correction again. If you turn on Learn from my fixes in Labs, Hex keeps the word pairs you corrected right after dictating, how often and when you last corrected each, on your phone only; it never sends them, and it learns nothing from password fields. Turning it off stops the learning but keeps the pairs until you tap Delete what Hex learned, which removes them. If you turn on Adaptive touch in Labs, Hex keeps, for each letter key, an average of where your taps land on it, in a table of fixed size (under 32 KB) on your phone only; it keeps no letters, words, or order of taps, never sends it, and learns nothing from password, email, or web address fields. Turning it off stops the learning but keeps the averages until you tap Reset touch learning or Delete what Hex learned, which remove them. If you turn on Clipboard history in Labs, Hex reads the text you copy while it is your keyboard and keeps the last 20 texts on your phone only, left out of backups and transfers to a new phone; each is deleted an hour after you copy it unless you pin it. If Android has stopped Hex by then, the text is deleted the next time the keyboard starts, and if the phone is asleep, when its screen next comes on. It never keeps text an app marks as sensitive, such as a password manager's, text copied while a password field is open, text Hex copied itself, or images and files, never sends them, and turning it off deletes them all. Clearing Hex's storage or uninstalling Hex removes all of it, and you can turn off microphone access in Android's settings.</li>
<li><strong>Hex Preview.</strong> <a href="${ANDROID_PATH}">Hex Preview</a>, the build you install outside Google Play, also asks GitHub for its latest version when you open it and at most once a day from the keyboard. When you tap Download update, your browser downloads the new build from GitHub. Those requests carry nothing about you or what you type; like any download, the server sees your IP address. Turn the check off with Check for updates automatically.</li>
</ul>
</section>
<section>
<h2>Hex on your computer</h2>
<p>Hex for Mac, Windows, and Linux also transcribes on your computer. This policy covers only Hex for iPhone and Android.</p>
</section>
<section>
<h2>Changes</h2>
<p>If this policy changes, we'll update this page and the date at the top.</p>
</section>
<section>
<h2>Contact</h2>
<p>Questions about privacy? <a href="${ISSUES}">Open an issue on GitHub</a>, or report a security problem privately through <a href="${SECURITY}">GitHub's vulnerability reporting</a>.</p>
</section>
</div>
<section class="about">
<p><a href="${IPHONE_PATH}">Hex for iPhone</a> · <a href="${ANDROID_PATH}">Hex for Android</a> · <a href="${withBase("/")}">Hex for your computer</a></p>
</section>`
  return shell(
    origin,
    {
      title: "Privacy policy · Hex for iPhone",
      description: "What you say and type stays on your iPhone: no account, no analytics, no tracking. Hex sends only the feedback you choose to send.",
      url: PRIVACY_PATH,
      image: PRIVACY_CARD_PATH,
      imageAlt: "Hex for iPhone privacy policy: what you say and type stays on your iPhone.",
    },
    body,
    { ...IPHONE, style: PROSE },
  )
}