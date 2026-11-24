# Data safety answers

For Hex for Android (`ly.anoma.hex`), including its input method and bundled libraries. Quoted definitions are from [Play’s Data safety guidance](https://support.google.com/googleplay/android-developer/answer/10787469).

Send feedback (Home › Send feedback, in every build with a report key, Play builds included) means Hex now “collects” in Play’s sense: “"Collect" means transmitting data from your app off a user’s device.” These answers replace the earlier “No data collected” before the first Play upload that contains it.

| Play declaration | Answer |
| --- | --- |
| Does the app collect or share any of the required user data types? | **Yes.** Collected only when the person writes feedback and taps Send. Nothing is shared. |
| Is all of the user data collected by your app encrypted in transit? | **Yes.** HTTPS only (`usesCleartextTraffic="false"`); the model download is HTTPS too. |
| Do you provide a way for users to request that their data is deleted? | **Decide before submitting.** Feedback is stored in Bellhop; answer Yes only with a deletion route (for example the policy’s contact) that Anomaly honours. |
| Account creation or account deletion URL | **No account creation.** |
| Independent security review or certification | **No claim.** |

## Data types

Every type below is **Collected: yes, Shared: no, Processed ephemerally: no, Optional** (“where a user chooses whether to manually provide that data type”: nobody has to send feedback to use Hex). Not shared: Bellhop is Anomaly’s own feedback inbox, and Play’s exemptions also cover a “service provider” and a “user-initiated action, where the user reasonably expects the data to be shared”.

| Play category › data type | Play’s definition | What Hex sends | Purposes |
| --- | --- | --- | --- |
| Personal info › **Email address** | “A user’s email address.” | `email`, only if typed into the form | App functionality, Developer communications |
| Messages › **Other in-app messages** | “Any other types of messages. For example, instant messages or chat content.” | `message`, `subject`, `category` | App functionality, Analytics |
| Photos and videos › **Photos** | “A user’s photos.” | The pictures kept in the form: Hex’s own window and any photos picked | App functionality, Analytics |
| App info and performance › **Diagnostics** | “Information about the performance of your app. For example battery life, loading time, latency, framerate, or any technical diagnostics.” | `app` (name, version, build), `device` (platform, Android version, model, locale), metadata (screen, channel, speech model, dictation language) | App functionality, Analytics |
| Device or other IDs › **Device or other IDs** | “Identifiers that relate to an individual device, browser or app. For example, an IMEI number, MAC address, Widevine Device ID, Firebase installation ID, or advertising identifier.” | `install_id`, a random UUID made by Hex, sent only with an email | App functionality, Analytics |

App activity › **Other user-generated content** (“Any other user-generated content not listed here, or in any other section. For example, user bios, notes, or open-ended responses.”) is the alternative home for the message; declare it as well if Play’s reviewers read the form as open-ended responses rather than messages. Purposes, as Play defines them: **App functionality** “Used for features that are available in the app”; **Analytics** “Used to collect data about how users use the app or how it performs”, whose examples include “to diagnose and fix bugs or crashes”; **Developer communications** “Used to send news or notifications about the app or the developer” (replies to the person’s feedback by email).

Not collected: audio, typed text in other apps, transcripts, vocabulary, corrections, location, contacts, `user` (Hex has no accounts) and `logs`. Without an email, feedback is anonymous (`"anonymous": true`: no email, install ID or user).

**Network:** Get started downloads one fixed, public 190 MB Whisper model from Hugging Face and its delivery network, with a generic user agent and no identifier, audio, text or usage events. Send feedback makes one `POST https://api.blhp.app/v1/feedback` when Send is tapped. The IME does not initiate network requests.

The microphone is used only when the person starts dictation. Audio is processed in memory and is never saved as a recording. Suggestions and corrections run on-device. Choosing to insert text into another app gives it the text; that app’s handling of it is separate.

**Privacy policy URL:** https://dev.anoma.ly/hex/privacy

**In-app text:** Settings → Privacy → Privacy policy, from [the Android policy](policy/privacy-policy.txt), which now has a “Feedback you send” section. The live page must say the same for Android before submission.
