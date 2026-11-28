# Typing configs: the format and the agent package

A typing config (`.mhexconfig`) is a file a person's own agent writes from their Hex typing
export, and Hex applies after showing and checking it. The format and every field are in
[package/reference/typing-config.md](package/reference/typing-config.md).

- `limits.json`: the one table of the format's limits. `ios/scripts/build-typing-config.py`
  generates the phone's clamps (`HEXShared/FullKeyboard/TypingConfig/TypingConfigLimits.swift`),
  the package's `scripts/limits.py` and `typing-config.schema.json` from it; `--check` fails
  when any of them is out of date.
- `typing-config.schema.json`: the JSON schema an agent's file is checked against (strict).
- `package/`: what the export carries besides the logs: `SKILL.md`, `reference/` and
  `scripts/` (Python 3 standard library only, no network).
