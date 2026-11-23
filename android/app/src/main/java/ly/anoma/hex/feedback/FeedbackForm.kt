package ly.anoma.hex.feedback

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.text.NumberFormat
import ly.anoma.hex.Card
import ly.anoma.hex.Label
import ly.anoma.hex.R
import ly.anoma.hex.Rule
import ly.anoma.hex.inter

/** Send feedback, opened from Home: a kind, a subject, a message, the pictures (Hex's window to start with,
 * removable, up to five with the photo picker), an optional email (none: anonymous) and what is sent, in plain
 * words. Nothing goes out until Send. */
@Composable
internal fun FeedbackForm(done: () -> Unit) {
    val phase = Feedback.phase
    if (phase is Feedback.Phase.Sent) return Thanks(phase, done)
    val context = LocalContext.current
    val focus = LocalFocusManager.current
    val draft = Feedback.draft
    val sending = phase is Feedback.Phase.Sending

    Card {
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).selectableGroup()
            .padding(horizontal = 10.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            FeedbackKind.entries.forEach { kind ->
                val chosen = Feedback.kind == kind
                Box(Modifier.clip(RoundedCornerShape(50)).background(colorResource(if (chosen) R.color.ink else R.color.sheet))
                    .selectable(chosen, enabled = !sending, role = Role.RadioButton) { Feedback.kind = kind; Feedback.edited() }
                    .heightIn(min = 36.dp).padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
                    Label(stringResource(kind.label), size = 15, weight = FontWeight.Medium, inverted = chosen)
                }
            }
        }
        Rule()
        Field(Feedback.subject, stringResource(R.string.feedback_subject), !sending, KeyboardOptions(
            capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Next)) {
            Feedback.subject = it.take(FeedbackLimits.SUBJECT); Feedback.edited()
        }
        Rule()
        Field(Feedback.message, stringResource(R.string.feedback_message), !sending,
            KeyboardOptions(capitalization = KeyboardCapitalization.Sentences), lines = 6) {
            Feedback.message = it; Feedback.edited()
        }
        if (Feedback.message.length > FeedbackLimits.MESSAGE - 1000) {
            val format = NumberFormat.getIntegerInstance()
            Label(stringResource(R.string.feedback_count, format.format(Feedback.message.length), format.format(FeedbackLimits.MESSAGE)),
                size = 13, muted = Feedback.message.length <= FeedbackLimits.MESSAGE,
                mod = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp))
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Label(stringResource(R.string.feedback_pictures), size = 14, weight = FontWeight.Medium, muted = true,
            mod = Modifier.padding(start = 4.dp))
        Pictures(enabled = !sending)
        if (Feedback.pictures.any { it.window })
            Label(stringResource(R.string.feedback_window_note), size = 13, muted = true, lines = 3, mod = Modifier.padding(horizontal = 4.dp))
    }

    Card {
        Field(Feedback.email, stringResource(R.string.feedback_email), !sending, KeyboardOptions(
            capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, keyboardType = KeyboardType.Email,
            imeAction = ImeAction.Done)) { Feedback.email = it.take(254); Feedback.edited() }
    }

    // The promise, in plain words: what goes out, to whom, and only on Send. No email: anonymous.
    Label(stringResource(if (draft.anonymous) R.string.feedback_sends_anonymous else R.string.feedback_sends_email),
        size = 13, muted = true, lines = Int.MAX_VALUE, mod = Modifier.padding(horizontal = 4.dp))

    val problem = when {
        phase is Feedback.Phase.Failed -> words(phase.failure)
        Feedback.tried || draft.problem == FeedbackDraft.Problem.MessageTooLong -> when (draft.problem) {
            FeedbackDraft.Problem.MessageTooLong -> R.string.feedback_message_too_long
            FeedbackDraft.Problem.PicturesTooLarge, FeedbackDraft.Problem.TooManyPictures -> R.string.feedback_pictures_too_large
            FeedbackDraft.Problem.InvalidEmail -> R.string.feedback_invalid_email
            else -> null
        }
        else -> null
    }
    problem?.let { Label(stringResource(it), size = 14, weight = FontWeight.Medium, lines = Int.MAX_VALUE, mod = Modifier.padding(horizontal = 4.dp)) }
    // The progress and any problem sit just above Send: keep them on screen.
    val send = remember { BringIntoViewRequester() }
    LaunchedEffect(sending, problem) { if (sending || problem != null) send.bringIntoView() }

    if (phase is Feedback.Phase.Sending) Column(verticalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(horizontal = 4.dp)) {
        LinearProgressIndicator(progress = { phase.fraction }, modifier = Modifier.fillMaxWidth(),
            color = colorResource(R.color.ink), trackColor = colorResource(R.color.rule))
        Label(if (phase.attempt > 1) stringResource(R.string.feedback_retrying)
            else stringResource(R.string.feedback_sending, (phase.fraction * 100).toInt()), size = 13, muted = true)
    }

    Button(stringResource(R.string.feedback_send), enabled = !sending && draft.message.isNotBlank(), Modifier.bringIntoViewRequester(send)) {
        focus.clearFocus()
        Feedback.send(context)
    }
}

@Composable
private fun Thanks(sent: Feedback.Phase.Sent, done: () -> Unit) {
    Label(when {
        sent.emailPending -> stringResource(R.string.feedback_thanks_email, sent.email)
        sent.email.isEmpty() -> stringResource(R.string.feedback_thanks_anonymous)
        else -> stringResource(R.string.feedback_thanks)
    }, muted = true, lines = Int.MAX_VALUE)
    Button(stringResource(R.string.setup_done), enabled = true, action = done)
}

@Composable
private fun Pictures(enabled: Boolean) {
    val context = LocalContext.current
    var unreadable by remember { mutableStateOf(false) }
    val room = FeedbackLimits.PICTURES - Feedback.pictures.size
    val picked: (List<android.net.Uri>) -> Unit = { uris ->
        if (uris.isNotEmpty()) Feedback.addPicked(context, uris) { unreadable = it > 0 }
    }
    // Android's photo picker needs no permission. Its multiple-choice form takes at least two.
    val pickMany = rememberLauncherForActivityResult(remember(room) { ActivityResultContracts.PickMultipleVisualMedia(room.coerceAtLeast(2)) }, picked)
    val pickOne = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { it?.let { uri -> picked(listOf(uri)) } }
    val request = PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Feedback.pictures.forEachIndexed { n, picture ->
            Box(Modifier.size(84.dp, 120.dp)) {
                Image(picture.thumbnail, stringResource(R.string.feedback_picture, n + 1),
                    Modifier.fillMaxSize().clip(RoundedCornerShape(12.dp)).border(1.dp, colorResource(R.color.rule), RoundedCornerShape(12.dp)),
                    contentScale = ContentScale.Crop)
                val remove = stringResource(R.string.feedback_remove_picture, n + 1)
                Box(Modifier.align(Alignment.TopEnd).size(44.dp).clickable(enabled, role = Role.Button) {
                    Feedback.pictures.remove(picture); Feedback.edited()
                }.semantics { contentDescription = remove }, contentAlignment = Alignment.TopEnd) {
                    Box(Modifier.padding(4.dp).size(24.dp).background(colorResource(R.color.ink), CircleShape), contentAlignment = Alignment.Center) {
                        Label("✕", size = 11, weight = FontWeight.Bold, inverted = true)
                    }
                }
            }
        }
        if (room > 0) {
            val add = stringResource(R.string.feedback_add_picture)
            Column(Modifier.size(84.dp, 120.dp).clip(RoundedCornerShape(12.dp)).background(colorResource(R.color.sheet))
                .clickable(enabled, role = Role.Button) {
                    unreadable = false
                    if (room >= 2) pickMany.launch(request) else pickOne.launch(request)
                }.semantics { contentDescription = add },
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
                Label("+", size = 22, muted = true)
                Label(stringResource(R.string.feedback_add), size = 13, muted = true)
            }
        }
    }
    if (unreadable) Label(stringResource(R.string.feedback_picture_unreadable), size = 13, lines = 2, mod = Modifier.padding(horizontal = 4.dp))
}

@Composable
private fun Field(value: String, hint: String, enabled: Boolean, options: KeyboardOptions, lines: Int = 1, change: (String) -> Unit) {
    val ink = colorResource(R.color.ink)
    BasicTextField(value = value, onValueChange = change, enabled = enabled, singleLine = lines == 1, minLines = lines,
        keyboardOptions = options, textStyle = TextStyle(color = ink, fontFamily = inter, fontSize = 16.sp, lineHeight = 21.sp),
        cursorBrush = SolidColor(ink), modifier = Modifier.fillMaxWidth().semantics { contentDescription = hint },
        decorationBox = { field ->
            Box(Modifier.heightIn(min = 52.dp).padding(horizontal = 16.dp, vertical = 15.dp)) {
                if (value.isEmpty()) Label(hint, muted = true, lines = 2)
                field()
            }
        })
}

@Composable
private fun Button(text: String, enabled: Boolean, mod: Modifier = Modifier, action: () -> Unit) {
    Box(mod.fillMaxWidth().height(52.dp).clip(RoundedCornerShape(14.dp)).background(colorResource(R.color.ink))
        .alpha(if (enabled) 1f else 0.4f).clickable(enabled, role = Role.Button, onClick = action),
        contentAlignment = Alignment.Center) { Label(text, inverted = true, weight = FontWeight.SemiBold) }
}

private val FeedbackKind.label get() = when (this) {
    FeedbackKind.Problem -> R.string.feedback_problem
    FeedbackKind.Idea -> R.string.feedback_idea
    FeedbackKind.Question -> R.string.feedback_question
    FeedbackKind.Other -> R.string.feedback_other
}

private fun words(failure: FeedbackFailure) = when (failure) {
    FeedbackFailure.Offline -> R.string.feedback_offline
    FeedbackFailure.TooLarge -> R.string.feedback_too_large
    FeedbackFailure.UnsupportedImage -> R.string.feedback_unsupported_image
    FeedbackFailure.TryLater -> R.string.feedback_try_later
    FeedbackFailure.Unavailable -> R.string.feedback_unavailable
    FeedbackFailure.Other -> R.string.feedback_failed
}
