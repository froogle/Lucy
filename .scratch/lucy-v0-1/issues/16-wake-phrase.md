# What is the default wake phrase, and how does a user get a different one?

Type: grilling
Status: resolved
Blocked by: none

## Question

Decide whether the default is "Lucy" or "Hey Lucy": a short single word that is also a common name risks false triggers from TV and conversation, and wake-word engines favour multi-word phrases. Then decide how users get their own word (a documented training recipe, a set of project-published models, or an optional sherpa-onnx "type any word" Provider), and who trains, hosts and reproduces the shipped model (training needs a CUDA GPU or Apple-silicon Mac plus a free LiveKit Cloud account for speech generation). See [Which local wake-word engine works from Rust for a custom word like "Lucy"?](01-wake-word-engine.md).

## Answer

- **Default wake phrase: "Hey Lucy".** Plain "Lucy" is about 4 phones and a common name, so it risks false triggers from TV and conversation, and an unwanted Session is worse than one extra syllable. The assistant is still called Lucy (GLOSSARY.md updated).
- **Changing the word in v0.1:** config takes a path to any LiveKit WakeWord v2 `.onnx` file. A short "train your own" doc covers LiveKit's recipe (GPU or Apple-silicon Mac plus a free LiveKit Cloud account). The project also publishes one extra model, plain "Lucy", for users who accept more false triggers. The wake-word engine is **not** a Provider: one fixed engine (`livekit-wakeword`) with a swappable model file. The sherpa-onnx "type any word" engine is deferred to the roadmap.
- **Training and hosting:** the maintainer trains the models by hand on a rented CUDA GPU (this dev machine is AMD and can't run the recipe). The `.onnx` files are committed to the repo under `models/wake/`, about 7 MB each, together with the exact training config, the command, the LiveKit WakeWord version and a README so anyone can reproduce them. CI training and release-asset hosting can be revisited under Publishing.

Follow-up: [Train and commit the "Hey Lucy" and "Lucy" wake-word models](27-train-wake-models.md) (blocks [Does the wake-word engine hold up over hours of normal desktop audio?](24-wake-word-soak.md)).
