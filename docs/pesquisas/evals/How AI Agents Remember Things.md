# How AI Agents Remember Things

Out of the box, AI agents have no memory. Every conversation starts with a blank slate. Most people think that you need a vector database, complex retrieval pipelines, or specialized memory to handle this. But OpenClaus solved it with markdown files and four mechanisms that fire at the right moments in a conversation. I'll show you exactly how it works.

AI models are inherently stateless. There's no memory between calls. Your conversation is an increasingly long context window that gets passed on each turn of the conversation. This is why without some kind of memory system, each new conversation starts without any context of the previous one. So how does an agent memory system actually work?

Memory systems can be broken up into two pieces, the session and longerterm memory. The session can be thought of as a history of a single conversation with an LLM. During the conversation, the state of it needs to be saved somewhere. This conversation history needs to be passed on each subsequent call you make to the LLM for it to remember and understand the current point of the conversation. There's a problem though.

LMS have a finite context window. Because of this, as you approach the limits of your context window, a process called compaction kicks in. Compaction is the act of taking the session's conversation history and breaking it down into the most important relevant information in order to allow the conversation to continue without losing all of the details of the session. There are three different strategies for triggering compaction. First is countbased.

This is when you compact a conversation once it's exceeded a certain token size or a certain turn count in the conversation. Second is timebased. This is triggered when the user stops interacting for a certain period of time. Compaction is then triggered in the background. Third is event-based or semantic.

Here an agent triggers compaction when it has detected that a particular task or topic has concluded. It's the most intelligent version of compaction, but also the most difficult to implement accurately because of the context window limit. We can't just persist and pass entire old conversations into a new conversation with an LLM. That is where long-term memory comes in. The memory is what survives at the end of a session.

Imagine a session as a messy desk for a current project. You might have various notes and documents scattered around your desk. Then you might have a filing cabinet where things are categorized and stored. This is the memory. There's a great framework for thinking about memory.

Google published a white paper in November of 2025 titled context engineering sessions and memory. In it, they break down agent memory into three types. The first type is episodic. Episodic memory covers things like what happened in our last conversation. These are events or interactions you might have had with the LM.

Second is semantic. Semantic memory are pure facts or user preferences. Think what do I the LM know about you the user or the topic. Third is procedural. Procedural memory covers things like workflows and learned routines or how do I accomplish this task.

All of these come together to form a memory for an agent. In order for the memory system to be effective, it must have a solid method of extracting key details from a conversation in order to persist them. Part of this is understanding what is worth remembering. Not every detail of a conversation is going to be important. Targeted filtering is needed in order for the memory to be effective.

Just like a human's memory where we may not remember full details of something. Instead, we remember key concepts and facts. In addition to this, it must also be able to consolidate items in memory. For example, imagine a user tells agent that I prefer dark mode in one conversation. In a later conversation, it says I don't like dark mode anymore.

And in another, it says I switched to dark mode. Without consolidation, all three entries sit in the memory saying essentially the same thing. A good memory system collapses those into a single entity. User prefers dark mode. It also must be able to overwrite previous decisions or determinations.

Something that might be true today may not necessarily be true tomorrow. The memory system must be able to differentiate and update its memory knowledge bank. Without this, the memory can become noisy and contradictory. These are both typically handled by another LLM instance that takes a conversation and handles this extraction and consolidation. There are different ways you can store memory from simple solutions such as markdown files for a local agent to specialized databases like vector storage that can be searched for relevant data when appropriate.

Let's take a look at a real world example of this system in place. Open clause memory model is a great example of agent memory in practice. I recently did a video explaining the underlying system of OpenClaw at a high level. Let's take a closer look at how its memory works. The OpenClaw memory system has three core components.

The first component is the memory MD file. This is the semantic memory store for Open Claw. It includes stable facts, preferences, and information about your identity. This is loaded into every single prompt and has a recommended 200line cap. The memory is broken up into structured sections.

Second are daily logs. Daily logs are one of open clause implementations of episodic memory. It contains recent context organized by day. These memory files are appended only which means that new memory entries are continually added but nothing gets removed. Third are session snapshots.

Session snapshots are the second implementation of episodic memory by openclaw. These are triggered by the session memory hook that fires when a new session is started via the slashnew or slashreset command. The snapshot captures the last 15 meaningful messages from your conversation. These are filtered to only user and assistant messages. That means things like tool calls and system messages and slash commands are all excluded.

It's not a generated summary, but it's actually the raw conversation text saved as a markdown file with a descriptive name. So at its core, open clause memory is just markdown files. That's straightforward enough, but it turns out that these files are only half the story. Without something that reads and writes them at the right times, they're just sitting there doing nothing. Remember the desk and the filing cabinet?

The files are the filing cabinet. And what we're about to look at are the mechanisms that move the things from the desk to the cabinet at the right moments. The first mechanism, bootstrap loading at the session start. For every new conversation, the memory MD is automatically injected into the prompt. The agent always has it.

On top of that, the agent's instructions tell it to read today and yesterday's daily logs for recent context. So the memory MD is injected by the system and the daily logs are loaded by the agent itself following its own instructions. This is the simplest pattern and the most important one. The second mechanism is the pre-ompaction flush. Open claw takes a count-based approach towards compaction.

When a session nears the context window limit, open claw injects a silent agentic turn that is invisible to the user. It instructs the LLM that you're near compaction and it needs to save anything that's important. Now, when the agent sees this message, it writes to the daily log. This serves as a checkpoint for the conversation. This turns a destructive operation such as losing context into a checkpoint.

It follows a common database pattern or write ahead log, saving memory before it's lost. The third mechanism fires when you start a new session, the session snapshot. Session snapshots are saved whenever a new session is started, whether that's via the /new or/reset command. As I mentioned in the previous video, sessions are per channel. A hook grabs the last chunk of your previous conversation, filters to meaningful messages only, and the LM generates a descriptive slug for the file name.

It's not a summary, it's a snapshot of what you were talking about, saved before the slate gets wiped. And finally, the simplest mechanism, the user just asks. If a user says something like remember this, the agent determines whether it belongs in the semantic memory or memory MD file or the daily log as episodic memory. No special hook is needed. The agent just has file writing capabilities and its instructions tell it how to route the information.

And that's it. Open Claw's entire memory system comes down to markdown files and knowing when to write to them. semantic memory in the memory MD file, episodic memory in daily logs and session snapshots, and four mechanisms that fire at the right moments in the conversation's life cycle. And these patterns aren't just an open claw. Claude Code recently shipped the memory feature, and it uses markdown files as well.

You don't need a complex setup to give an agent memory. You just need clear instructions to three questions. What's worth remembering? Where does it go? And when does it get written?

If you want to go deeper into agent architecture and patterns like this, I write a weekly newsletter covering AI augmented engineering and building with LLM. Link is in the description.