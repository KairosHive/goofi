# AI

Language models, speech, vision foundation models and the patch's coding agents as nodes. Every provider is reached through
pydantic-ai where it has a layer for the task, and through the provider's own SDK where it does
not; a key is the provider's own environment variable (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
`GEMINI_API_KEY`, `XAI_API_KEY`, `TYPESAFE_API_KEY`, `ELEVENLABS_API_KEY`), and a local server that
speaks the OpenAI API stands in through `base_url`. The bundle runs on the subprocess tier.

## LLM

One reply per text frame on `prompt` after `system_prompt`, with the latest `data` frame appended
as numbers. `model` is `provider:model` (`openai:gpt-6-luna`, `anthropic:claude-sonnet-4-5`,
`google:gemini-2.5-flash`, `ollama:llama3.2`, …); `effort` sets how hard it thinks. `single`
answers each prompt alone; `chat` keeps the last `history` prompts and replies until
`clear_history`, and always the system prompt. `require_change` skips a prompt whose text and data
equal the last answered request's.

## Decision

Which of the possible answers on `options` (one per line, each `label` or `label: what it means`)
a text frame on `text` calls for, by a decision model (`openai-decisions:gpt-6-luna`,
`typesafe:jev-latest`, `system-one:<model>`) that answers `question` with probabilities rather than
text. `answer` is the picked label and `probabilities` one per label.

## Agent

A coding agent in the patch workspace. `start` launches `harness` with `model` and `effort` where
the harness takes them; `instance` steers a running one instead, whoever started it. Every text
frame on `prompt` is typed into its terminal and submitted. The instance shows in the agent panel
like any other.

## VoiceAgent

A live call with a speech-to-speech model (`openai:gpt-realtime`, `google:gemini-live-2.5-flash`,
`xai:grok-voice-latest`). Wire `signal:AudioIn` (stream mode) into `audio` and `audio` into
`audio:SignalIn`; the voice comes back in step with the frames that arrive. The model's tool
`command_agent` leaves its text on `command`: link that to an `Agent` node's `prompt`. A text frame
on `text` is a typed turn.

## TextToSpeech, SpeechToText

Text frames spoken as 24 kHz audio frames, and audio frames transcribed in `segment`-second pieces,
by OpenAI, ElevenLabs or an OpenAI-compatible server.

## DINO

DINOv3 on the GPU, over a texture on `image` (a camera, a video, a renderer): `embedding` is the
class token (384 to 4096 numbers by `size`), `patches` the normalized patch features as
`[rows, columns, dim]`, both from the last layer. The frame is resized on its short side to
`resolution` and run in bf16 through SDPA attention; `compile` adds CUDA graphs and fuses the PCA
after a minute of warm-up. Meta's Hugging Face repos are gated: accept the DINOv3 license
on the repo page and `hf auth login` once, or set `repo` to an ungated mirror in the transformers
format. `segment` mode adds the self-supervised segmentation of the DINOv2 paper: a PCA of the
patch features whose first component, above `threshold`, is the foreground (`mask`), and a PCA of
the foreground patches whose three components are the colours of `map`, both at the picture's
own size. `memory` carries the PCA basis and the foreground side from frame to frame so neither
flips; `invert` swaps foreground and background when the border of the picture is the subject.
