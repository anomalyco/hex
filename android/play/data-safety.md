# Data safety answers

For Hex for Android (`ly.anoma.hex`), including its input method and bundled libraries:

| Play declaration | Answer |
| --- | --- |
| Does the app collect or share any required user data types? | **No.** No user data collected; no user data shared. |
| Data types selected | **None.** This includes audio, typed text, transcripts, app activity, diagnostics, location and device or other IDs. |
| Is data encrypted in transit? | If shown, **yes** for the model transfer (HTTPS only). There is no user-content upload. |
| Account creation or account deletion URL | **No account creation.** No account-deletion URL applies. |
| Data deletion | There is no server-side user data to delete. Clearing Hex’s Android app storage or uninstalling it removes its local settings, correction counts and downloaded model. |
| Independent security review or certification | **No claim.** |

**Why the internet permission is present:** Get started downloads one fixed, public 190 MB Whisper model from Hugging Face and its delivery network. The service necessarily sees the source IP to return the file. Hex sends a generic user agent, no app or device identifier, no audio, text, transcript or usage events. A failed download can be retried; an installed model is reused. The bundled tiny model also works without the download. The IME does not initiate network requests.

The microphone is used only when the person starts dictation. Audio is processed in memory and is never saved as a recording. Suggestions and corrections run on-device. Choosing to insert text into another app gives it the text; that app’s handling of it is separate.

**Privacy policy URL:** https://dev.anoma.ly/hex/privacy

**In-app text:** Settings → Privacy → Privacy policy, from [the Android policy](policy/privacy-policy.txt). The live page currently says it covers only iPhone; add the Android text to the same URL before submission.

[Play’s Data safety definitions](https://support.google.com/googleplay/android-developer/answer/10787469)
