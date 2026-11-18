# Lucy

A voice-driven desktop assistant for Linux that listens for its name, converses, and carries out actions on the user's machine and accounts.

## Language

**Lucy**:
The assistant as a whole, as experienced by the user. Her name is part of the default Wake word.
_Avoid_: Jarvis, bot, agent (when meaning the whole system)

**Wake word**:
The spoken phrase that opens a Session. "Hey Lucy" by default; the user can swap in another (for example plain "Lucy").
_Avoid_: Hotword, trigger word, activation phrase

**Session**:
One continuous spoken exchange between the user and Lucy, opened by the Wake word and ended by the user or by silence.
_Avoid_: Conversation, chat, interaction

**Turn**:
One thing the user says within a Session and Lucy's whole response to it, including any actions she takes.
_Avoid_: Exchange, message, round

**Barge-in**:
The user cutting in while Lucy is speaking or thinking. Lucy stops and listens to what they say instead.
_Avoid_: Interrupt (as a noun for the user's action), interjection

**Closing phrase**:
Whatever the user says to end a Session ("that's all, thanks", "bye Lucy"). It is understood from meaning, not matched against a fixed list.
_Avoid_: Stop word, exit command

**Indicator**:
The small on-screen marker shown for the whole of a Session; clicking it ends the Session.
_Avoid_: Overlay, widget, badge

**Provider**:
A pluggable source of one capability (speech-to-text, thinking, or speech) that the user configures from whatever they already have.
_Avoid_: Backend, plugin (when meaning a Provider), integration

**Voice preview**:
Lucy speaking a sentence she is given, outside any Session, so a person can hear her voice and speaker. A Session that opens cuts it short.
_Avoid_: Say command, TTS test

**Plugin**:
A self-contained ability Lucy can use, described by a Manifest. Whether Lucy wrote it herself or a person did, it is the same thing.
_Avoid_: Routine, skill, extension, macro

**Manifest**:
A Plugin's short description of what requests it can handle and what it needs access to; Lucy reads Manifests to decide which Plugin fits a request.
_Avoid_: Descriptor, spec

**Approval**:
The user's yes or no that lets a new or changed Plugin be used. Every Plugin needs one, whoever wrote it, and any change to it needs a fresh one.
_Avoid_: Confirmation (that is the spoken yes before a risky action), permission, consent

**Confirmation**:
The user's spoken yes before a risky action, asked and heard by Lucy herself rather than by the thinking model. Most cover one action; the yes to the screen covers every screen action until lucyd restarts. Anything other than a clear yes means no.
_Avoid_: Approval (that is the yes for a whole Plugin), permission

**Input dialog**:
A box Lucy puts on screen so the user can type an answer she shouldn't hear or can't easily catch, such as a password, an API key or a long URL. A secret typed there goes only to the tool or Plugin that asked, never to the model.
_Avoid_: prompt (when meaning an Input dialog), popup, form

**Built-in tool**:
An action Lucy provides herself rather than through a Plugin, such as opening a link or launching an app. Built-in tools need no Approval, so there are only a few of them.
_Avoid_: Core plugin, native tool

**Model tier**:
One of two roles a thinking Provider can fill: the fast tier (understanding requests, choosing Plugins, everyday conversation) or the advanced tier (research, writing Plugins, open-ended exploration).
_Avoid_: Model class, level (when meaning a Model tier)

**Delegate**:
An autonomous agent Lucy hands a goal to and lets work on its own, reporting progress and asking questions along the way, such as the advanced tier building a Plugin.
_Avoid_: Sub-agent, worker, Agent B

**Authoring**:
Lucy writing a new Plugin, or fixing one of her own, by handing the goal to a Delegate. It ends in an Approval, and the Delegate never touches the user's data before that.
_Avoid_: Plugin generation, building (as the name for the whole process)

**Hand-off**:
A Delegate that runs the user's own Claude, with their own config and integrations, on a request they asked Lucy to pass on ("tell Claude to …"). Unlike Authoring, it is not sandboxed: what it may do is what the user's own Claude settings allow, and anything else needs a Confirmation.
_Avoid_: forward, relay (when meaning a Hand-off)

**Persona**:
The personality and manner of speaking of one user's Lucy, including what she calls them. Each user shapes their own, by editing it or by telling her.
_Avoid_: Character, personality profile, system prompt

**Source**:
A news site the news briefing reads through its official feed. Lucy summarises only Sources whose terms allow it; for the rest she offers to open the site's page.
_Avoid_: Outlet, feed (the feed is how a Source is read), publisher
