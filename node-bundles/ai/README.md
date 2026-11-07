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

DINO on the GPU, over a texture on `image` (a camera, a video, a renderer): `embedding` is the
class token (384 to 4096 numbers by `size`), `patches` the normalized patch features as
`[rows, columns, dim]`, both from the last layer. The frame is resized on its short side to
`resolution` and run in bf16 through SDPA attention; `compile` adds CUDA graphs after a minute of
warm-up. A picture is encoded only when its texels move, so a still picture costs no GPU time
after its first frame. Meta's Hugging Face repos are gated: accept the DINOv3 license on the repo
page and `hf auth login` once, or set `repo` to an ungated mirror in the transformers format.
`segment` mode adds the picture on the front of the DINOv3 repo: `map` is the cosine similarity
between the patch under `point` (a share of the width and height) and every other patch, a
continuous field at the picture's own size that follows whatever the point sits on, and `mask` is
that field above `threshold`. Drive `point` from a pose or a cursor and the map tracks what is
pointed at; a higher `resolution` gives a finer field. `tokens` mode encodes the grid a
`DinoDecode` draws, one token for each 16 x 16 pixels of a drawing `resolution` on its short
side: `space` is `dinov2` (DINOv2-B with registers, its last layer, MIT) or `dinov3-k23` and
`dinov3-k7` (DINOv3 ViT-L, the mean of 23 or 7 of its blocks).

## DinoDecode

The grid of tokens on `tokens` — a `DINO` node's `tokens`, or anything made from such grids — is
drawn as a picture of `rows * 16` by `columns * 16` pixels by the decoder trained on the token
space the meta names: RAE's ViT-XL for `dinov2` (halfway between two pictures it draws one new
surface, MIT) or RAEv2's for `dinov3-k23` and `dinov3-k7` (they keep the photographs, CC BY-NC
4.0). The grid is drawn in one pass at any size, the decoder's position table interpolated to it,
and a grid equal to the last one drawn is answered with the last drawing. Each decoder is 1.7 GB
from the hub on first use. For a morph between pictures, hold each one's grid and wire them all
into a `Morph`, whose `position` glides between them, then into `DinoDecode`; an `Operation` or a
`Math` between the two nodes mixes or moves the latent instead.
