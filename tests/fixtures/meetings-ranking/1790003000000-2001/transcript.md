# Asset pipeline sync

**00:00 Them** Morning. I made a short agenda for today.

**00:10 You** Great. Bob made the slides for the review, I believe.

**00:20 Them** I made a blog post about the launch last night.

**00:30 You** Nice, I believe the blog post is already getting traffic.

**00:40 Them** Bob said he made the onboarding video too.

**00:50 You** We made good progress on the editor this week.

**01:00 Them** Bob is out on Friday, so plan around that.

**01:10 You** Who made the decision about the pricing page?

**01:20 Them** I believe it was the two of us, honestly.

**01:30 You** Fine. The search box still feels slow on large projects.

**01:40 Them** Let's profile it after lunch.

**01:50 You** Okay, next topic is where uploads should live.

**02:00 Them** We should keep the images out of the database and put them in blob storage instead.

**02:10 You** Right, the media files are huge, so blob storage with a CDN in front makes sense.

**02:20 Them** Then the database only keeps a key for each image plus its media type.

**02:30 You** Thumbnails for the images can sit in the same bucket, generated on upload.

**02:40 Them** And the storage costs stay predictable because old renders expire.

**02:50 You** Good. Back to the blog: who reviews the next post?

**03:00 Them** Bob made a list of reviewers, I believe he shared it.

**03:10 You** Also someone made a mess of the staging deploy yesterday.

**03:20 Them** That was me, I made a typo in the config.

**03:30 You** No worries. Separately, someone asked whether the old blob left over from the prototype can be deleted, and I said yes once the migration finishes next week. Let's wrap up and write the notes.
