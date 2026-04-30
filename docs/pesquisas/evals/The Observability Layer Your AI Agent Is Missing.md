# The Observability Layer Your AI Agent Is Missing

AI agents aren't like the software you're used to monitoring. They don't just follow instructions, they make decisions. And that fundamental difference means that the tools you've relied on to understand your systems don't necessarily transfer cleanly. You need a different approach to observability. One built around decision-making, not just execution.

I'm Damien, and I've been building software for over 15 years. And recently, I've been running a multi-agent system. My agent Emma handles my business operations. Invoicing, CRM updates, scheduling, and client communications. Everything I'm about to show you comes from debugging that system.

To see inside a running agent, you need three things. Logs, traces, and metrics. This video breaks down what each one gives you and why you need all three. Observability for an agent means being able to see inside its decision-making process. Not just whether it ran, but why it did what it did.

That starts with the logs, the foundation of any observability practice. They're the raw record. Every event, every tool call, every model response, timestamped and stored. But a log entry is isolated. It tells you what happened at that moment.

It doesn't tell you why one step led to the next, what the agent was reasoning about, what context it was carrying, what made the tool call the logical next move. Logs give you the individual moments. What you're missing is the thread that connects them. To make this concrete, let me walk you through a real example. I've been experimenting with giving Emma the ability to handle invoicing via the Stripe's agent toolkit.

To receive agent call ledger. I was onboarding a new consulting client, so I asked Emma in Slack to draft an invoice. Have me review it and then send it. She pulled the client contacts from the CRM, delegated to ledger, and came back with a clean draft. Line items, amount, payment terms, everything.

I reviewed it in the Stripe dashboard, and everything looked good. So, I asked Emma to send it. Emma replied that the invoice was sent successfully with the invoice ID, the amount, and the payment URL. Everything looked good. 3 hours later, I realized the client never actually got the invoice.

It was finalized, which means it was moved from draft to open, but never actually sent. In Stripe, those are two different states. The invoice was sitting there open with no email delivered. When I checked the logs, the Slack message was there. The tool calls were there.

There were no errors, no 500s, nothing indicating that something went wrong. If you looked at the logs, every piece of the system said the task was done. The logs did contain the answer, though. Every tool call was recorded. If I'd gone digging, I could have found that ledger called the finalize invoice tool and never the send invoice tool.

But finding it meant scrolling a flat feed of tool calls across three different agents. And then noticing the absence of a call I'd have to already know should have existed. And then piecing together why Emma interpreted the finalization as delivery. The logs had the data, but what I needed was the story behind it all. Logs tell you what happened, traces tell you why.

A trace isn't a more detailed log, it's a different shape entirely. The shape is that of a tree. The root is the top-level task, the thing that the user asked for. Every decision, every tool call, every sub-agent delegation becomes a node in the tree called a span. A named timed unit of work with its own inputs, outputs, and status.

The connections between spans show you why, not just what the agent did, but how it decided to do it. When something goes wrong, you don't scroll a feed, you walk down the tree. Back to the invoice. I collect traces from my agents using Arize Phoenix, an open-source LLM tracing and evaluation platform. When I open the trace for that send request, I saw Emma at the root.

Her first few spans looked normal, reading the request, deciding to delegate to ledger. Ledger becomes a child span, and I expand it. Ledger's subtree shows the work that it did for that send request. There's a span for looking up the invoice, a span for checking its status. Then ledger calls finalize invoice.

That span succeeded. But there's no next step, no send invoice call. The tree just stops. In a log feed, a missing call is invisible. It's the absence of a line.

And you'd have to already know it should be there. In a trace tree, you can see everything that ledger did and can see exactly where it stopped. Next, I walked back up the tree to Emma. Her next span is the tool call where she composes the Slack response. I can read the input, ledger's result with the status of open.

And I can read the output, invoice sent successfully. That's where the trace gave me something to work with. Not necessarily an answer, but a clear thread to follow. Emma had read open as completion, which meant that the question became what did ledger actually return and why did it stop where it did? That reasoning step where the model receives an input, interprets it, and generates an output is also a span that you can read in the trace.

In logs, it doesn't exist. There's no log entry for model interpreted this field to mean X. The logs had the data, the trace had the story. One is an archaeology project, while the other is a narrative that you can read. If you've instrumented production applications before, none of this is necessarily new.

Traces aren't a new concept. OpenTelemetry has been the standard for observability across cloud-native software for years. The same primitives, the same tooling, now applied to agent decision chains. The primitives map directly. The spans, trace IDs, the parent-child relationships, it's all the same vocabulary, just applied to a different object.

Instead of tracing a request as it moves through services, you're tracing a task as it's executed by an agent. The structure is the same. What you're looking at is different. The tooling exists and works. Arize Phoenix, LangFuse, BrainTrust, Mastra, they all speak OpenTelemetry.

They all render the tree. What's new isn't necessarily the infrastructure, it's what you're looking at. Not a request path, but a decision chain. Once you have the trace, debugging changes shape. You're not searching through events trying to reconstruct what happened.

The story is already written. You can open it, walk down the tree, and find the scene where the plot diverged. The leverage isn't more data, it's a better story. Debugging one trace tells you what went wrong in one task, but at scale, you're running thousands. That's where metrics come in.

Logs and traces answer questions about one task, one run, one invoice. Metrics answer questions about all of them, computed over the windows of data, aggregated across a thousands of runs. But metrics are derived. They're only as good as the layer underneath them. Metrics built on logs can only tell you system-level stories.

Metrics built on traces can tell you the story. There are two categories of metrics. The first is what your dashboard already shows you. The health of infrastructure running your agent. The second is what tells you whether the agent is actually doing its job.

System metrics include latency, error rate, token cost, uptime. These watch the server that hosts your agent. During Emma's invoice failure, every one of them was green. Latency was normal. The HTTP response from the Slack API came back as a 200.

There was nothing for a system metric to flag. Quality metrics tell you the agent is making good decisions. Correctness, trajectory adherence, whether the output actually matched the task required. These are second order. You don't read them off a raw span, but you compute them by running evals against your trace data.

If you're not familiar with evals, I covered that framework in part one. I'll link the video here. System metrics watch the machine, while quality metrics watch the agent itself. If you're only watching the system metrics, you're not watching the agent. You're watching the server that the agent runs on.

This is where individual traces can become a data set. One trace tells you what went wrong in one task. Quality metrics computed across thousands of traces tell you how your agent is behaving overall. Whether correctness is declining after a deploy, and whether certain task types are consistently underperforming. Whether the agent is drifting from the behavior you intended.

I found that failure because I happened to check 3 hours later by chance. Without quality metrics, that's how agent failures get discovered after the fact. Manually, when someone notices something soft. Aggregate metrics would have caught this pattern across every run, not just the one I stumbled onto. The evals framework only works if you can see what your agent did.

Quality metrics are computed from trace data. Evals run on quality metrics. The entire measurement loop, the thing that tells you whether your agent is actually getting better, depends on the observability layer underneath it. You can only measure what you can see. Traces are how you see it.

In the next video, I'll walk you through what this looks like in practice. Real code, real traces, and evals tied together with the observability layer. If you found this video useful, give it a like and subscribe so you don't miss the next one. I also have a weekly newsletter where I write about building with AI. Link is in the description.

And if you're working through problems like this one on a team, I can help with that, too. Everything's linked below. I'll see you in the next one.