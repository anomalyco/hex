//! `hex mcp`: a read-only MCP stdio server over recorded meetings. A thin
//! adapter; queries and output live in `meeting::query`.

use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ListResourceTemplatesResult, ListResourcesResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, ResourceTemplate, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;

use crate::events::now_ms;
use crate::meeting::query::{Library, ListQuery, OffsetRange, SearchQuery, TranscriptQuery};

const RESOURCE_SCHEME: &str = "hex-meeting://";

const INSTRUCTIONS: &str = "Kit's locally recorded Hex meetings (calls captured on his Mac). \
Speakers are `You` (Kit, his microphone) and `Them` (everyone else, system audio); individual \
remote speakers are not separated. Transcripts come from local speech recognition and contain \
misheard words, so search is stemmed and fuzzy by default. Typical flow: list_meetings or \
search_transcripts to find a meeting and timestamp, then get_transcript around it, and \
get_meeting_notes for a stored title and two-sentence summary, and get_chapters for the topics \
a meeting moved through with their time spans, when it has any. Details, decisions, and action \
items are only in the transcript. Everything is read-only and stays on the machine.";

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListMeetingsArgs {
    /// Only meetings overlapping this time or later: `2026-10-15`, `2026-10-15T14:00`,
    /// RFC 3339, `today`, `yesterday`, or relative `7d`/`36h`.
    #[serde(default)]
    pub since: Option<String>,
    /// Only meetings starting before this time; a date alone includes that whole day.
    #[serde(default)]
    pub until: Option<String>,
    /// Words that must all appear in the title or app (case- and accent-insensitive).
    #[serde(default)]
    pub query: Option<String>,
    /// Maximum meetings to return (default 20, maximum 200).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Include a meeting that is being recorded right now (default true).
    #[serde(default)]
    pub include_in_progress: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchTranscriptsArgs {
    /// Words to find. BM25-ranked; tolerant of misspellings and sound-alikes unless
    /// fuzzy is false. Wrap words in double quotes to require an exact phrase.
    pub query: String,
    /// Only meetings overlapping this time or later (same formats as list_meetings).
    #[serde(default)]
    pub since: Option<String>,
    /// Only meetings starting before this time.
    #[serde(default)]
    pub until: Option<String>,
    /// Search a single meeting.
    #[serde(default)]
    pub meeting_id: Option<String>,
    /// Maximum passages to return (default 10, maximum 50).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Match prefixes, small misspellings, and sound-alike names (default true).
    #[serde(default)]
    pub fuzzy: Option<bool>,
    /// Include a meeting that is being recorded right now (default true).
    #[serde(default)]
    pub include_in_progress: Option<bool>,
}

/// A transcript offset: `mm:ss`, `h:mm:ss`, or seconds.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Offset {
    Seconds(f64),
    Text(String),
}

impl Offset {
    fn text(self) -> String {
        match self {
            Self::Seconds(seconds) => seconds.to_string(),
            Self::Text(text) => text,
        }
    }

    /// One bound given under either of its two names.
    fn either(
        (name, value): (&str, Option<Self>),
        (alias, other): (&str, Option<Self>),
    ) -> Result<Option<String>> {
        match (value, other) {
            (Some(_), Some(_)) => Err(eyre!("Give either {name} or {alias}, not both.")),
            (value, other) => Ok(value.or(other).map(Self::text)),
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTranscriptArgs {
    /// The meeting id from list_meetings or search_transcripts.
    pub meeting_id: String,
    /// Start offset into the meeting: `mm:ss`, `h:mm:ss`, or seconds.
    /// Entries overlapping start–end are returned whole.
    #[serde(default)]
    pub start: Option<Offset>,
    /// End offset into the meeting: `mm:ss`, `h:mm:ss`, or seconds.
    #[serde(default)]
    pub end: Option<Offset>,
    /// Same as `start`; give one or the other.
    #[serde(default)]
    pub from: Option<Offset>,
    /// Same as `end`; give one or the other.
    #[serde(default)]
    pub to: Option<Offset>,
    /// `you`, `them`, or `all` (default).
    #[serde(default)]
    pub speaker: Option<String>,
    /// Maximum transcript characters to return (default 20000).
    #[serde(default)]
    pub max_chars: Option<u32>,
    /// `next_cursor` from a previous call, to continue where it stopped.
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetMeetingNotesArgs {
    /// The meeting id from list_meetings or search_transcripts.
    pub meeting_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetChaptersArgs {
    /// The meeting id from list_meetings or search_transcripts.
    pub meeting_id: String,
}

#[derive(Clone)]
pub struct MeetingServer {
    library: Arc<Library>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for MeetingServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MeetingServer")
            .field("root", &self.library.root())
            .finish()
    }
}

fn reply(result: Result<String>) -> Result<CallToolResult, ErrorData> {
    Ok(match result {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
    })
}

impl MeetingServer {
    /// The saved vocabulary, read on each search so edits apply without a
    /// restart.
    fn vocabulary(&self) -> Vec<crate::vocabulary::VocabularyEntry> {
        crate::app_paths::support_dir()
            .map(|root| crate::vocabulary::read_saved(&root))
            .unwrap_or_default()
    }
}

#[tool_router]
impl MeetingServer {
    pub fn new(root: PathBuf) -> Self {
        Self {
            library: Arc::new(Library::new(root)),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List recorded meetings, newest first: id, title (generated title when \
present), start time, duration, status, transcript kind, participants, and whether notes exist.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn list_meetings(
        &self,
        Parameters(args): Parameters<ListMeetingsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        reply(self.library.list(
            &ListQuery {
                since: args.since,
                until: args.until,
                query: args.query,
                limit: args.limit.unwrap_or(20) as usize,
                include_in_progress: args.include_in_progress.unwrap_or(true),
            },
            now_ms(),
        ))
    }

    #[tool(
        description = "Full-text search across meeting transcripts and stored notes. Returns \
ranked passages with meeting id, title, speaker, mm:ss offset, absolute time, and a snippet \
with surrounding lines, plus match counts per meeting. Case- and accent-insensitive, stemmed, \
and fuzzy for speech-recognition errors (Dax/DAX/Dacks).",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn search_transcripts(
        &self,
        Parameters(args): Parameters<SearchTranscriptsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        reply(self.library.search(
            &SearchQuery {
                query: args.query,
                since: args.since,
                until: args.until,
                meeting_id: args.meeting_id,
                limit: args.limit.unwrap_or(10) as usize,
                fuzzy: args.fuzzy.unwrap_or(true),
                include_in_progress: args.include_in_progress.unwrap_or(true),
                vocabulary: self.vocabulary(),
            },
            now_ms(),
        ))
    }

    #[tool(
        description = "Read a meeting transcript as merged You/Them turns with mm:ss offsets and \
local clock times. Optionally limit to a time range (start/end, or the same as from/to, as mm:ss, \
h:mm:ss, or seconds; entries overlapping it are included, and a start not before the end or after \
the meeting's end is an error) or one speaker. Long results stop at max_chars and return next_cursor to \
continue.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn get_transcript(
        &self,
        Parameters(args): Parameters<GetTranscriptArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let start = Offset::either(("start", args.start), ("from", args.from));
        let end = Offset::either(("end", args.end), ("to", args.to));
        reply(start.and_then(|start| {
            self.library.transcript(
                &TranscriptQuery {
                    meeting_id: args.meeting_id,
                    start,
                    end: end?,
                    speaker: args.speaker,
                    max_chars: args.max_chars.unwrap_or(20_000) as usize,
                    cursor: args.cursor,
                },
                now_ms(),
            )
        }))
    }

    #[tool(
        description = "The stored Apple Intelligence title and summary (at most two sentences) \
for a meeting. Only a short overview: read the transcript with get_transcript or \
search_transcripts for details, decisions, and action items. Never generates notes; when none \
are stored it says so.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn get_meeting_notes(
        &self,
        Parameters(args): Parameters<GetMeetingNotesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        reply(self.library.notes(&args.meeting_id))
    }

    #[tool(
        description = "A meeting's chapters: the topics it moved through, each with a short \
model-written name, one line, and its mm:ss start and end, for reading one part with \
get_transcript. Only meetings recorded with live chapters have them. Never generates chapters; \
when none are stored it says so.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    fn get_chapters(
        &self,
        Parameters(args): Parameters<GetChaptersArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        reply(self.library.chapters(&args.meeting_id, now_ms()))
    }
}

fn internal(error: color_eyre::Report) -> ErrorData {
    ErrorData::internal_error(error.to_string(), None)
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MeetingServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(
            Implementation::new("hex-meetings", crate::VERSION).with_title("Hex Meetings"),
        )
        .with_instructions(INSTRUCTIONS)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let scan = self.library.scan().map_err(internal)?;
        Ok(ListResourcesResult::with_all_items(
            scan.meetings
                .iter()
                .map(|meeting| {
                    Resource::new(
                        format!("{RESOURCE_SCHEME}{}", meeting.id),
                        meeting.id.clone(),
                    )
                    .with_title(meeting.title())
                    .with_description(format!(
                        "Meeting transcript and notes, started {}",
                        crate::meeting::query::iso_local(meeting.start_ms())
                    ))
                    .with_mime_type("text/markdown")
                })
                .collect(),
        ))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new(format!("{RESOURCE_SCHEME}{{meeting_id}}"), "meeting")
                .with_title("Meeting Markdown")
                .with_description("A meeting's metadata, stored notes, and transcript")
                .with_mime_type("text/markdown"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let Some(id) = request.uri.strip_prefix(RESOURCE_SCHEME) else {
            return Err(ErrorData::resource_not_found(
                format!("unknown resource {}", request.uri),
                None,
            ));
        };
        let markdown = self
            .library
            .markdown(id.trim_end_matches('/'), OffsetRange::default(), now_ms())
            .map_err(|error| ErrorData::resource_not_found(error.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(markdown, request.uri.clone()).with_mime_type("text/markdown"),
        ])
        .into())
    }
}

/// Serves MCP over stdin/stdout until the client disconnects. Reads only
/// `root`; logs go to stderr so stdout carries protocol messages alone.
pub fn run(root: PathBuf) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let service = MeetingServer::new(root)
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|error| eyre!("MCP session failed to start: {error}"))?;
        service.waiting().await?;
        Ok(())
    })
}
