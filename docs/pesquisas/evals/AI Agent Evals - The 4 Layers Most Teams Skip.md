# AI Agent Evals: The 4 Layers Most Teams Skip

How to know if your AI agents actually work? When you're building AI agents, the hardest part isn't getting them to work. It's knowing whether they actually do. You spend time building your agent, you test it yourself, maybe [snorts] with a few different scenarios, and things look good. So, you ship it.

But, running it and checking the output isn't the same as knowing it works. There's a specific practice that's emerged for exactly this. It's called evals, and it works differently than the testing you already know. Agents don't fail in the same way that deterministic software fails. Things can seem fine when in fact they are not.

The output can look correct, the logs can look clean, but the agent made the wrong decision somewhere in the middle. Running it a few times and checking the output will catch the obvious problems, but it won't tell you whether the agent is making good decisions, and that's a harder problem to solve. Unit tests are built around one assumption. Given the same input, you'll get the same output every time. &gt;&gt; [snorts] &gt;&gt; Agents don't work that way.

Integration tests assume predictable interfaces, but an agent's interface is natural language. There's no schema to assert against. End-to-end tests assume a fixed happy path. An agent might take three steps or 25 to reach the same goal. The natural instinct is to reach for testing tools you already know.

The problem is those tools weren't built for this kind of system. So, they don't fit, what does? The word eval sounds like grading, and grading sounds like a pass or fail. But, that's not quite what this is. You can think of it more like a manufacturing QA.

You don't test one widget at a time. You sample the batch and measure the defect rate over time. An eval works the same way. It scores on a spectrum, zero to one, across dozens or even hundreds of inputs. A response could be 0.7 correct.

You're not asking, "Did it pass?" You're tracking whether quality is holding, improving, or degrading across every change that you make. That's essentially what evals are, CI for probabilistic systems. In practice, three things make this work. First, you start with a benchmark set, a curated collection of inputs with known good reference outputs. Next is scoring functions, automated judges that evaluate each response against your criteria.

Lastly, there's tracking over time. You look at trends rather than snapshots. That's the framework, but what specifically do you score? There are four layers to think about, from the component level all the way up to the system level. The first is the component layer, your tools and functions.

These are deterministic. You can test them normally with unit tests. A tool that parses JSON either parses it correctly or it doesn't. Your existing testing instincts can work fine here. The second is trajectory.

Did the agent take the right steps? Did it select the right tools, construct the right parameters, follow a reasonable reasoning chain? An agent that gets the right answer in 25 tool calls when three would do has a trajectory problem. And an agent that calls the wrong tool but happens to get the right answer, that's also a trajectory problem, but it will likely become an outcome problem. &gt;&gt; [snorts] &gt;&gt; The third layer is the outcome.

Is the final answer from the agent correct, helpful, grounded, and complete? This is the hardest layer to evaluate because those questions are subjective. You can't write an assertion for helpful. That's where LLM as judge comes in. The idea is straightforward.

You use second language model to evaluate your agent's output. You give it a set of criteria that defines what a good answer looks like for this task, and it scores each response against that rubric. The division of labor here is humans define what good means. The model applies that definition at scale. You get the judgment of a careful reviewer without the cost of reviewing everything manually.

That said, automated evals don't catch everything. Regularly reading the production traces directly, reading the actual inputs and outputs, surfaces the subtle failures that no rubric has anticipated. The fourth layer is system monitoring, watching for quality degrading in production at scale. Not individual failures, but patterns across real usage over time. This [snorts] is where evals observability start to overlap, and it's where the next video will pick up.

There's a specific order to how to use these layers. You want to start from the outside in. Start with the outcome. If it fails, you open the box. But, even when it passes, trajectory is worth monitoring.

An agent that takes 25 steps when three would do is leaving efficiency and reliability problems on the table. Now, the layers tell you where to measure, but they don't tell you what quality actually means. For that, there are four dimensions. Effectiveness, did the agent achieve what the user actually wanted? That's the baseline.

Efficiency, did it do it well? There's a real difference between 25 tool calls and three, between 10 seconds and two minutes. And token costs add up fast when your agent is taking unnecessary steps. Robustness, does it hold up under pressure? Malformed input, API failures, ambiguous instructions, and edge cases.

And lastly, safety and alignment. Does it stay within the bounds? Does it refuse to do things when it should refuse? That one's a non-negotiable. Which brings up the real challenge.

You can only measure what you can see. You can't measure effectiveness if you only see the final answer. You can't measure efficiency if you don't count the steps. You can't diagnose a robustness failure if you don't know which API call failed. Quality requires visibility into what the agent is actually doing.

And that visibility has to be designed in, not added later. &gt;&gt; [snorts] &gt;&gt; Most teams build the agent, ship it, and then ask, "How do we test this?" That's the wrong order. If your agent doesn't emit structured traces, you can't evaluate trajectory. If it doesn't log tool calls with parameters, you can't measure efficiency. If it doesn't expose intermediate reasoning, you can't diagnose failures.

Designing this from the start is an architectural decision. The same way that observability and security is. You build it from day one. Design the agent so quality is measurable, then measure it continuously. There's one more dimension to this.

What happens over time? Every production failure is a data point. When you capture it and annotate it, it becomes a regression test. The pattern looks like this. Production failures surface problems.

You annotate them as eval cases. The eval set grows, and the agent improves. New edge cases surface, and you repeat. The eval set becomes a living record of everything your agent has struggled with, real failures, real edge cases, real user interactions that didn't go well. Over time, it's the most accurate picture you have of what your agent actually needs to handle.

You don't build quality in one pass. You build it incrementally, and it compounds. To recap, evals aren't tests. They're a way of measuring quality over time using scores and distributions rather than a simple pass or fail. Agent quality has four layers and four dimensions.

You measure from the outside in. And quality is something you design for from day one, not something that you add after the fact. If you want to know where your system actually stands, I put together a one-page scorecard based on the four layers we've covered. Score yourself on each one, and you'll know exactly where your gaps are. Link is in the description.

The next video in this series covers observability, traces, metrics, and what it looks like when you can actually see inside a running agent. And if you found this useful, subscribe so you don't miss the next one. In the meantime, you can find more videos on building AI agents here.