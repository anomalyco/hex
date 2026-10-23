#!/usr/bin/env python3
"""Writes tests/fixtures/live-notes-replay.ndjson: a synthetic twelve-minute
call with four topics, for the live meeting notes replay preview."""
import json
import pathlib

TOPICS = [
    [
        ("Them", "Okay, I think everyone's here. Should we start with the first-run flow for Lantern?"),
        ("You", "Yeah, let's do onboarding first. Right now new players land in the hub with no idea what to do."),
        ("Them", "The playtest numbers back that up. About forty percent quit before they finish the first puzzle."),
        ("You", "That's rough. I think the tutorial room is too long. It explains every mechanic before you can touch anything."),
        ("Them", "What if the first puzzle teaches the lantern itself, and we hold back the mirrors until room three?"),
        ("You", "I like that. Light first, mirrors later. Players learn by doing instead of reading."),
        ("Them", "We'd need to cut the narrator lines in the tutorial, though. Most of them assume mirrors already."),
        ("You", "That's fine. The narrator is the weakest part anyway. Let's keep two lines and drop the rest."),
        ("Them", "Do we still want the skip button for returning players?"),
        ("You", "Yes, definitely keep skip. Returning players hated replaying it in the last build."),
        ("Them", "Okay, so we agreed: lantern-only first room, mirrors in room three, two narrator lines, keep skip."),
        ("You", "Agreed. I'll rebuild the first room this week and send a build to the playtest group on Friday."),
        ("Them", "Great. One open question is whether co-op players share a lantern in the tutorial or each get one."),
        ("You", "Good question. Sharing forces them to talk, but it might be frustrating if one person hogs it."),
        ("Them", "Let's test both in the Friday build and look at the recordings."),
        ("You", "Sounds good. Two variants, shared and separate lanterns."),
        ("Them", "And we should measure time to first puzzle solved, not just quit rate."),
        ("You", "Yeah, time to first solve is the real number. I'll add that event to the analytics."),
        ("Them", "Perfect. I think that covers onboarding for now."),
        ("You", "Cool. Should we move on to the multiplayer stuff? That's the scary part."),
    ],
    [
        ("Them", "So the lobby is still dropping players when the host migrates. It happened six times in yesterday's session."),
        ("You", "Is that the relay server or our own host migration code?"),
        ("Them", "Mostly ours. When the host leaves, the new host doesn't get the puzzle state fast enough and clients time out."),
        ("You", "What's the timeout right now? Five seconds?"),
        ("Them", "Three seconds, and the state snapshot is almost two megabytes because it includes every light ray."),
        ("You", "We don't need the rays. Clients can recompute them from the mirror positions."),
        ("Them", "True, the rays are deterministic. If we only send mirror angles and lantern positions, the snapshot is tiny."),
        ("You", "Let's do that. Send the inputs, recompute the light locally."),
        ("Them", "There's a risk of drift if floating point differs across platforms."),
        ("You", "We could quantize the mirror angles to whole degrees. Puzzles only use forty five degree steps anyway."),
        ("Them", "That works. Quantized angles, snapshot of inputs only. We decided, then?"),
        ("You", "Yes, decided. And bump the timeout to ten seconds as a safety net."),
        ("Them", "I can take the snapshot rewrite. Maybe three days of work."),
        ("You", "Great, you own the snapshot change. I'll write a soak test that kills the host every two minutes."),
        ("Them", "Do we keep the relay server, or move to direct connections for friends?"),
        ("You", "Keep the relay for launch. Direct connections are a post launch question."),
        ("Them", "What about voice chat in the lobby? Players keep asking."),
        ("You", "Out of scope for launch. Platform voice chat covers most people."),
        ("Them", "Fine by me. Less to break."),
        ("You", "Exactly. Okay, the art pass next? I saw the new lighting shots."),
    ],
    [
        ("Them", "Yeah, Maya sent the new lighting pass for the caves this morning. It looks gorgeous."),
        ("You", "It does, but the frame rate on the Steam Deck dropped to twenty two in the big cave."),
        ("Them", "That's the volumetric fog. Every lantern adds a fog volume and they stack up."),
        ("You", "Can we cap the number of fog volumes, like four at a time, and fade the rest?"),
        ("Them", "We could, but four player co-op means at least four lanterns plus the world lights."),
        ("You", "Then make world lights skip fog on low settings. Players care about their own lantern most."),
        ("Them", "That's a good trade. Player lanterns keep fog, world lights don't on low."),
        ("You", "And can we bake the cave bounce lighting instead of computing it live?"),
        ("Them", "Most of it, yes. The moving mirrors are the problem since they redirect light."),
        ("You", "Bake everything static and keep only mirror light dynamic. That should get us back to forty."),
        ("Them", "I'll ask Maya to split the static and dynamic lights in the cave scenes."),
        ("You", "Thanks. Do we have a performance budget written down anywhere?"),
        ("Them", "Not really. It's in people's heads, which is how we got here."),
        ("You", "Let's write one. Forty frames on Deck, sixty on mid range PCs, measured in the three heaviest scenes."),
        ("Them", "I can draft that doc today and put it in the wiki."),
        ("You", "Perfect. Who checks it on each build though?"),
        ("Them", "Maybe the nightly build can run the three scenes and post the numbers in the channel."),
        ("You", "Love it. Automated perf capture on nightly. I'll set up the capture script."),
        ("Them", "Should the art style change at all for performance, or just the tech?"),
        ("You", "Just the tech. The look is what sells the game. Okay, last thing, the launch date."),
    ],
    [
        ("Them", "So the publisher wants a date by next Wednesday. They're pushing for early March."),
        ("You", "Early March is tight with the netcode work and the perf pass. Late March feels honest."),
        ("Them", "If we say March twenty sixth, that gives us three weeks of polish after content lock."),
        ("You", "And content lock would be the first week of March?"),
        ("Them", "March fifth for content lock, March twenty sixth for launch. That's my proposal."),
        ("You", "I'm okay with that if the soak test passes by content lock. Otherwise we slip."),
        ("Them", "Agreed. Launch on March twenty sixth, conditional on the soak test passing by March fifth."),
        ("You", "Who tells the publisher?"),
        ("Them", "I'll email them tomorrow with the date and the condition."),
        ("You", "Good. We also need the store page trailer. Is that still on Maya?"),
        ("Them", "Maya's busy with lighting now. Maybe we hire the trailer out?"),
        ("You", "Let's get two quotes this week and decide Monday."),
        ("Them", "I'll reach out to the studio that did the reveal trailer and one more."),
        ("You", "What about the demo for the festival? Is that happening?"),
        ("Them", "Still open. It depends on whether onboarding is fixed in time."),
        ("You", "Then the Friday playtest decides the demo too. If time to first solve looks good, we ship the demo."),
        ("Them", "Makes sense. So the open questions are the trailer vendor and the festival demo."),
        ("You", "Yep. I think that's everything. Thanks everyone, good meeting."),
        ("Them", "Thanks, talk Friday."),
    ],
]

entries = []
clock_ms = 4_000
for topic in TOPICS:
    for speaker, text in topic:
        words = len(text.split())
        duration = int(words / 2.6 * 1000)
        entries.append({
            "source": "microphone" if speaker == "You" else "system",
            "start_ms": clock_ms,
            "end_ms": clock_ms + duration,
            "text": text,
        })
        clock_ms += duration + 1_400
    clock_ms += 4_000

out = pathlib.Path(__file__).resolve().parents[2] / "tests/fixtures/live-notes-replay.ndjson"
out.write_text("".join(json.dumps(entry) + "\n" for entry in entries))
print(f"{out}: {len(entries)} entries, {clock_ms / 60000:.1f} minutes")
