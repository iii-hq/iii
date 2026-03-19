import { useState, useEffect, useCallback } from "react";
import { Highlight, themes } from "prism-react-renderer";
import { BrainCircuitIcon, CopyIcon, CheckedIcon } from "../icons";

interface AgentReadySectionProps {
  isDarkMode?: boolean;
}

type Lang = "typescript" | "python" | "rust";

const LangIcon = ({ lang, active }: { lang: Lang; active: boolean }) => {
  const opacity = active ? 1 : 0.5;
  const size = "w-4 h-4";

  if (lang === "typescript") {
    return (
      <svg viewBox="0 0 24 24" className={size} style={{ opacity }}>
        <rect width="24" height="24" rx="2" fill="#3178c6" />
        <path
          d="M1.125 0C.502 0 0 .502 0 1.125v21.75C0 23.498.502 24 1.125 24h21.75c.623 0 1.125-.502 1.125-1.125V1.125C24 .502 23.498 0 22.875 0zm17.363 9.75c.612 0 1.154.037 1.627.111a6.38 6.38 0 0 1 1.306.34v2.458a3.95 3.95 0 0 0-.643-.361 5.093 5.093 0 0 0-.717-.26 5.453 5.453 0 0 0-1.426-.2c-.3 0-.573.028-.819.086a2.1 2.1 0 0 0-.623.242c-.17.104-.3.229-.393.374a.888.888 0 0 0-.14.49c0 .196.053.373.156.529.104.156.252.304.443.444s.423.276.696.41c.273.135.582.274.926.416.47.197.892.407 1.266.628.374.222.695.473.963.753.268.279.472.598.614.957.142.359.214.776.214 1.253 0 .657-.125 1.21-.373 1.656a3.033 3.033 0 0 1-1.012 1.085 4.38 4.38 0 0 1-1.487.596c-.566.12-1.163.18-1.79.18a9.916 9.916 0 0 1-1.84-.164 5.544 5.544 0 0 1-1.512-.493v-2.63a5.033 5.033 0 0 0 3.237 1.2c.333 0 .624-.03.872-.09.249-.06.456-.144.623-.25.166-.108.29-.234.373-.38a1.023 1.023 0 0 0-.074-1.089 2.12 2.12 0 0 0-.537-.5 5.597 5.597 0 0 0-.807-.444 27.72 27.72 0 0 0-1.007-.436c-.918-.383-1.602-.852-2.053-1.405-.45-.553-.676-1.222-.676-2.005 0-.614.123-1.141.369-1.582.246-.441.58-.804 1.004-1.089a4.494 4.494 0 0 1 1.47-.629 7.536 7.536 0 0 1 1.77-.201zm-15.113.188h9.563v2.166H9.506v9.646H6.789v-9.646H3.375z"
          fill="white"
        />
      </svg>
    );
  }

  if (lang === "python") {
    return (
      <svg viewBox="0 0 24 24" className={size} style={{ opacity }}>
        <path
          d="M14.25.18l.9.2.73.26.59.3.45.32.34.34.25.34.16.33.1.3.04.26.02.2-.01.13V8.5l-.05.63-.13.55-.21.46-.26.38-.3.31-.33.25-.35.19-.35.14-.33.1-.3.07-.26.04-.21.02H8.77l-.69.05-.59.14-.5.22-.41.27-.33.32-.27.35-.2.36-.15.37-.1.35-.07.32-.04.27-.02.21v3.06H3.17l-.21-.03-.28-.07-.32-.12-.35-.18-.36-.26-.36-.36-.35-.46-.32-.59-.28-.73-.21-.88-.14-1.05-.05-1.23.06-1.22.16-1.04.24-.87.32-.71.36-.57.4-.44.42-.33.42-.24.4-.16.36-.1.32-.05.24-.01h.16l.06.01h8.16v-.83H6.18l-.01-2.75-.02-.37.05-.34.11-.31.17-.28.25-.26.31-.23.38-.2.44-.18.51-.15.58-.12.64-.1.71-.06.77-.04.84-.02 1.27.05zm-6.3 1.98l-.23.33-.08.41.08.41.23.34.33.22.41.09.41-.09.33-.22.23-.34.08-.41-.08-.41-.23-.33-.33-.22-.41-.09-.41.09zm13.09 3.95l.28.06.32.12.35.18.36.27.36.35.35.47.32.59.28.73.21.88.14 1.04.05 1.23-.06 1.23-.16 1.04-.24.86-.32.71-.36.57-.4.45-.42.33-.42.24-.4.16-.36.09-.32.05-.24.02-.16-.01h-8.22v.82h5.84l.01 2.76.02.36-.05.34-.11.31-.17.29-.25.25-.31.24-.38.2-.44.17-.51.15-.58.13-.64.09-.71.07-.77.04-.84.01-1.27-.04-1.07-.14-.9-.2-.73-.25-.59-.3-.45-.33-.34-.34-.25-.34-.16-.33-.1-.3-.04-.25-.02-.2.01-.13v-5.34l.05-.64.13-.54.21-.46.26-.38.3-.32.33-.24.35-.2.35-.14.33-.1.3-.06.26-.04.21-.02.13-.01h5.84l.69-.05.59-.14.5-.21.41-.28.33-.32.27-.35.2-.36.15-.36.1-.35.07-.32.04-.28.02-.21V6.07h2.09l.14.01zm-6.47 14.25l-.23.33-.08.41.08.41.23.33.33.23.41.08.41-.08.33-.23.23-.33.08-.41-.08-.41-.23-.33-.33-.23-.41-.08-.41.08z"
          fill="#3776AB"
        />
      </svg>
    );
  }

  return (
    <svg viewBox="0 0 24 24" className={size} style={{ opacity }}>
      <path
        d="M23.8346 11.7033l-1.0073-.6236a13.7268 13.7268 0 00-.0283-.2936l.8656-.8069a.3483.3483 0 00-.1154-.578l-1.1066-.414a8.4958 8.4958 0 00-.087-.2856l.6904-.9587a.3462.3462 0 00-.2257-.5446l-1.1663-.1894a9.3574 9.3574 0 00-.1407-.2622l.49-1.0761a.3437.3437 0 00-.0274-.3361.3486.3486 0 00-.3006-.154l-1.1845.0416a6.7444 6.7444 0 00-.1873-.2268l.2723-1.153a.3472.3472 0 00-.417-.4172l-1.1532.2724a14.0183 14.0183 0 00-.2278-.1873l.0415-1.1845a.3442.3442 0 00-.49-.328l-1.076.491c-.0872-.0476-.1742-.0952-.2623-.1407l-.1903-1.1673A.3483.3483 0 0016.256.955l-.9597.6905a8.4867 8.4867 0 00-.2855-.086l-.414-1.1066a.3483.3483 0 00-.5781-.1154l-.8069.8666a9.2936 9.2936 0 00-.2936-.0284L12.2946.1683a.3462.3462 0 00-.5892 0l-.6236 1.0073a13.7383 13.7383 0 00-.2936.0284L9.9803.3374a.3462.3462 0 00-.578.1154l-.4141 1.1065c-.0962.0274-.1903.0567-.2855.086L7.744.955a.3483.3483 0 00-.5447.2258L7.009 2.348a9.3574 9.3574 0 00-.2622.1407l-1.0762-.491a.3462.3462 0 00-.49.328l.0416 1.1845a7.9826 7.9826 0 00-.2278.1873L3.8413 3.425a.3472.3472 0 00-.4171.4171l.2713 1.1531c-.0628.075-.1255.1509-.1863.2268l-1.1845-.0415a.3462.3462 0 00-.328.49l.491 1.0761a9.167 9.167 0 00-.1407.2622l-1.1662.1894a.3483.3483 0 00-.2258.5446l.6904.9587a13.303 13.303 0 00-.087.2855l-1.1065.414a.3483.3483 0 00-.1155.5781l.8656.807a9.2936 9.2936 0 00-.0283.2935l-1.0073.6236a.3442.3442 0 000 .5892l1.0073.6236c.008.0982.0182.1964.0283.2936l-.8656.8079a.3462.3462 0 00.1155.578l1.1065.4141c.0273.0962.0567.1914.087.2855l-.6904.9587a.3452.3452 0 00.2268.5447l1.1662.1893c.0456.088.0922.1751.1408.2622l-.491 1.0762a.3462.3462 0 00.328.49l1.1834-.0415c.0618.0769.1235.1528.1873.2277l-.2713 1.1541a.3462.3462 0 00.4171.4161l1.153-.2713c.075.0638.151.1255.2279.1863l-.0415 1.1845a.3442.3442 0 00.49.327l1.0761-.49c.087.0486.1741.0951.2622.1407l.1903 1.1662a.3483.3483 0 00.5447.2268l.9587-.6904a9.299 9.299 0 00.2855.087l.414 1.1066a.3452.3452 0 00.5781.1154l.8079-.8656c.0972.0111.1954.0203.2936.0294l.6236 1.0073a.3472.3472 0 00.5892 0l.6236-1.0073c.0982-.0091.1964-.0183.2936-.0294l.8069.8656a.3483.3483 0 00.578-.1154l.4141-1.1066a8.4626 8.4626 0 00.2855-.087l.9587.6904a.3452.3452 0 00.5447-.2268l.1903-1.1662c.088-.0456.1751-.0931.2622-.1407l1.0762.49a.3472.3472 0 00.49-.327l-.0415-1.1845a6.7267 6.7267 0 00.2267-.1863l1.1531.2713a.3472.3472 0 00.4171-.416l-.2713-1.1542c.0628-.0749.1255-.1508.1863-.2278l1.1845.0415a.3442.3442 0 00.328-.49l-.49-1.076c.0475-.0872.0951-.1742.1407-.2623l1.1662-.1893a.3483.3483 0 00.2258-.5447l-.6904-.9587.087-.2855 1.1066-.414a.3462.3462 0 00.1154-.5781l-.8656-.8079c.0101-.0972.0202-.1954.0283-.2936l1.0073-.6236a.3442.3442 0 000-.5892zm-6.7413 8.3551a.7138.7138 0 01.2986-1.396.714.714 0 11-.2997 1.396zm-.3422-2.3142a.649.649 0 00-.7715.5l-.3573 1.6685c-1.1035.501-2.3285.7795-3.6193.7795a8.7368 8.7368 0 01-3.6951-.814l-.3574-1.6684a.648.648 0 00-.7714-.499l-1.473.3158a8.7216 8.7216 0 01-.7613-.898h7.1676c.081 0 .1356-.0141.1356-.088v-2.536c0-.074-.0536-.0881-.1356-.0881h-2.0966v-1.6077h2.2677c.2065 0 1.1065.0587 1.394 1.2088.0901.3533.2875 1.5044.4232 1.8729.1346.413.6833 1.2381 1.2685 1.2381h3.5716a.7492.7492 0 00.1296-.0131 8.7874 8.7874 0 01-.8119.9526zM6.8369 20.024a.714.714 0 11-.2997-1.396.714.714 0 01.2997 1.396zM4.1177 8.9972a.7137.7137 0 11-1.304.5791.7137.7137 0 011.304-.579zm-.8352 1.9813l1.5347-.6824a.65.65 0 00.33-.8585l-.3158-.7147h1.2432v5.6025H3.5669a8.7753 8.7753 0 01-.2834-3.348zm6.7343-.5437V8.7836h2.9601c.153 0 1.0792.1772 1.0792.8697 0 .575-.7107.7815-1.2948.7815zm10.7574 1.4862c0 .2187-.008.4363-.0243.651h-.9c-.09 0-.1265.0586-.1265.1477v.413c0 .973-.5487 1.1846-1.0296 1.2382-.4576.0517-.9648-.1913-1.0275-.4717-.2704-1.5186-.7198-1.8436-1.4305-2.4034.8817-.5599 1.799-1.386 1.799-2.4915 0-1.1936-.819-1.9458-1.3769-2.3153-.7825-.5163-1.6491-.6195-1.883-.6195H5.4682a8.7651 8.7651 0 014.907-2.7699l1.0974 1.151a.648.648 0 00.9182.0213l1.227-1.1743a8.7753 8.7753 0 016.0044 4.2762l-.8403 1.8982a.652.652 0 00.33.8585l1.6178.7188c.0283.2875.0425.577.0425.8717zm-9.3006-9.5993a.7128.7128 0 11.984 1.0316.7137.7137 0 01-.984-1.0316zm8.3389 6.71a.7107.7107 0 01.9395-.3625.7137.7137 0 11-.9405.3635z"
        fill="#dea584"
      />
    </svg>
  );
};

const capabilities = [
  {
    name: "AI Agent with Tools",
    code: {
      typescript: `import { registerWorker, Logger } from "iii-sdk"
const iii = registerWorker(process.env.III_BRIDGE_URL ?? "ws://localhost:49134")
const logger = new Logger()

const tools = await iii.listFunctions()

iii.registerFunction(
  { id: "agent::research" },
  async ({ query }) => {
    const response = await callLLM(query, { tools })

    while (response.toolCall) {
      const result = await iii.trigger({
        function_id: response.toolCall.function,
        payload: response.toolCall.args
      })
      logger.info("Tool used", { tool: response.toolCall.function })
      response = await callLLM(query, { tools, toolResult: result })
    }
    return response
  }
)`,
      python: `from iii import register_worker, Logger

iii = register_worker(os.environ.get("III_BRIDGE_URL", "ws://localhost:49134"))

async def research_handler(input):
    logger = Logger()
    tools = await iii.list_functions()
    response = await call_llm(input["query"], tools=tools)

    while response.get("tool_call"):
        tc = response["tool_call"]
        result = await iii.trigger({'function_id': tc["function"], 'payload': tc["args"]})
        logger.info("Tool used", tool=tc["function"])
        response = await call_llm(input["query"], tools=tools, tool_result=result)
    return response

iii.register_function("agent::research", research_handler)`,
      rust: `use iii_sdk::{register_worker, InitOptions, Logger, TriggerRequest};

let iii = register_worker("ws://localhost:49134", InitOptions::default())?;
let logger = Logger::new();
let tools = iii.list_functions().await?;

iii.register_function(
    RegisterFunctionInput { id: "agent::research".into(), ..Default::default() },
    |input| async move {
        let mut response = call_llm(&input.query, &tools).await?;

        while let Some(tool_call) = &response.tool_call {
            let result = iii.trigger(TriggerRequest::new(&tool_call.function, &tool_call.args)).await?;
            logger.info("Tool used", &tool_call.function);
            response = call_llm(&input.query, &tools, Some(result)).await?;
        }
        Ok(response)
    },
)?;`,
    },
  },
  {
    name: "Multi-Agent Network",
    code: {
      typescript: `import { TriggerAction } from "iii-sdk"

iii.registerFunction({ id: "agents::researcher" }, async ({ topic }) => {
  const sources = await iii.trigger({ function_id: "tools::webSearch", payload: { query: topic } })
  return iii.trigger({ function_id: "agents::analyzer", payload: { sources, topic } })
})

iii.registerFunction({ id: "agents::analyzer" }, async ({ sources, topic }) => {
  const insights = await callLLM("Analyze these sources", { sources })
  return iii.trigger({ function_id: "agents::writer", payload: { insights, topic } })
})

iii.registerFunction({ id: "agents::writer" }, async ({ insights, topic }) => {
  const draft = await callLLM("Write a report", { insights })
  await iii.trigger({ function_id: "state::set", payload: {
    scope: "reports", key: topic, value: draft
  } })
  iii.trigger({ function_id: "publish", payload: { topic: "report.ready", data: { topic } }, action: TriggerAction.Void() })
  return draft
})`,
      python: `from iii import TriggerAction

async def researcher(input):
    sources = await iii.trigger({"function_id": "tools::webSearch", "payload": {"query": input["topic"]}})
    return await iii.trigger({"function_id": "agents::analyzer", "payload": {"sources": sources, "topic": input["topic"]}})

async def analyzer(input):
    insights = await call_llm("Analyze these sources", sources=input["sources"])
    return await iii.trigger({"function_id": "agents::writer", "payload": {"insights": insights, "topic": input["topic"]}})

async def writer(input):
    draft = await call_llm("Write a report", insights=input["insights"])
    await iii.trigger({"function_id": "state::set", "payload": {
        "scope": "reports", "key": input["topic"], "value": draft
    }})
    await iii.trigger({"function_id": "publish", "payload": {"topic": "report.ready", "data": {"topic": input["topic"]}}, "action": TriggerAction.Void()})
    return draft

iii.register_function("agents::researcher", researcher)
iii.register_function("agents::analyzer", analyzer)
iii.register_function("agents::writer", writer)`,
      rust: `use iii_sdk::{TriggerRequest, TriggerAction};

iii.register_function(
    reg("agents::researcher"), |input| async move {
        let sources = iii.trigger(TriggerRequest::new("tools::webSearch", json!({"query": input.topic}))).await?;
        iii.trigger(TriggerRequest::new("agents::analyzer", json!({"sources": sources, "topic": input.topic}))).await
    },
)?;

iii.register_function(
    reg("agents::analyzer"), |input| async move {
        let insights = call_llm("Analyze these sources", &input.sources).await?;
        iii.trigger(TriggerRequest::new("agents::writer", json!({"insights": insights, "topic": input.topic}))).await
    },
)?;

iii.register_function(
    reg("agents::writer"), |input| async move {
        let draft = call_llm("Write a report", &input.insights).await?;
        iii.trigger(TriggerRequest::new("state::set", json!({
            "scope": "reports", "key": input.topic, "value": draft
        }))).await?;
        iii.trigger(TriggerRequest::new("publish", json!({"topic": "report.ready"})).action(TriggerAction::void())).await?;
        Ok(draft)
    },
)?;`,
    },
  },
  {
    name: "Durable Workflows",
    code: {
      typescript: `iii.registerFunction({ id: "orders::process" }, async ({ orderId }) => {
  const logger = new Logger()

  const step = await iii.trigger({ function_id: "state::get", payload: {
    scope: orderId, key: "step"
  } }) ?? 0

  const pipeline = [
    () => iii.trigger({ function_id: "payments::charge", payload: { orderId } }),
    () => iii.trigger({ function_id: "inventory::reserve", payload: { orderId } }),
    () => iii.trigger({ function_id: "shipping::create", payload: { orderId } }),
    () => iii.trigger({ function_id: "notifications::send", payload: { orderId } }),
  ]

  for (let i = step; i < pipeline.length; i++) {
    await pipeline[i]()
    await iii.trigger({ function_id: "state::set", payload: {
      scope: orderId, key: "step", value: i + 1
    } })
    logger.info("Step completed", { orderId, step: i + 1 })
  }
  return { status: "completed" }
})`,
      python: `async def process_order(input):
    logger = Logger()
    order_id = input["orderId"]

    step = await iii.trigger({"function_id": "state::get", "payload": {
        "scope": order_id, "key": "step"
    }}) or 0

    pipeline = [
        lambda: iii.trigger({"function_id": "payments::charge", "payload": {"orderId": order_id}}),
        lambda: iii.trigger({"function_id": "inventory::reserve", "payload": {"orderId": order_id}}),
        lambda: iii.trigger({"function_id": "shipping::create", "payload": {"orderId": order_id}}),
        lambda: iii.trigger({"function_id": "notifications::send", "payload": {"orderId": order_id}}),
    ]

    for i in range(step, len(pipeline)):
        await pipeline[i]()
        await iii.trigger({"function_id": "state::set", "payload": {
            "scope": order_id, "key": "step", "value": i + 1
        }})
        logger.info("Step completed", order_id=order_id, step=i + 1)
    return {"status": "completed"}

iii.register_function("orders::process", process_order)`,
      rust: `use iii_sdk::TriggerRequest;

iii.register_function(
    reg("orders::process"), |input| async move {
        let logger = Logger::new();
        let order_id = &input.order_id;

        let step: usize = iii.trigger(TriggerRequest::new("state::get", json!({
            "scope": order_id, "key": "step"
        }))).await.unwrap_or(0);

        let pipeline = [
            "payments::charge", "inventory::reserve",
            "shipping::create", "notifications::send",
        ];

        for (i, func) in pipeline.iter().enumerate().skip(step) {
            iii.trigger(TriggerRequest::new(func, json!({"orderId": order_id}))).await?;
            iii.trigger(TriggerRequest::new("state::set", json!({
                "scope": order_id, "key": "step", "value": i + 1
            }))).await?;
            logger.info("Step completed", &format!("{}: {}", order_id, i + 1));
        }
        Ok(json!({"status": "completed"}))
    },
)?;`,
    },
  },
  {
    name: "Polyglot Workers",
    code: {
      typescript: `import { registerWorker, TriggerAction } from "iii-sdk"
const iii = registerWorker(process.env.III_BRIDGE_URL ?? "ws://localhost:49134")

iii.registerFunction({ id: "api::users" }, async (req) => {
  const user = await db.createUser(req)
  iii.trigger({ function_id: "publish", payload: { topic: "user.created", data: user }, action: TriggerAction.Void() })
  return user
})

iii.registerTrigger({
  type: "http",
  function_id: "api::users",
  config: { api_path: "users", http_method: "POST" }
})

iii.registerTrigger({
  type: "subscribe",
  function_id: "ml::onboarding",
  config: { topic: "user.created" }
})`,
      python: `from iii import register_worker, Logger

iii = register_worker(os.environ.get("III_BRIDGE_URL", "ws://localhost:49134"))

async def predict_handler(input):
    logger = Logger()
    model = load_model("onboarding-v3")
    score = model.predict(input["user"])
    logger.info("ML prediction", score=score)
    return {"score": score, "segment": classify(score)}

async def recommend_handler(input):
    embeddings = await get_embeddings(input["user"])
    return vector_db.search(embeddings, top_k=10)

iii.register_function("ml::onboarding", predict_handler)
iii.register_function("ml::recommend", recommend_handler)`,
      rust: `use iii_sdk::{register_worker, InitOptions, TriggerRequest};

let iii = register_worker("ws://localhost:49134", InitOptions::default())?;

iii.register_function(
    reg("transform::images"), |input| async move {
        let image = decode_image(&input.data)?;
        let resized = image.resize(800, 600, FilterType::Lanczos3);
        let compressed = encode_webp(&resized, 85)?;
        iii.trigger(TriggerRequest::new("storage::upload", json!({
            "key": input.key, "data": compressed
        }))).await?;
        Ok(json!({"size": compressed.len()}))
    },
)?;

iii.register_function(
    reg("transform::video"), |input| async move {
        let frames = extract_frames(&input.data, 30)?;
        Ok(json!({"frames": frames.len()}))
    },
)?;`,
    },
  },
  {
    name: "Real-Time Streaming",
    code: {
      typescript: `iii.registerFunction({ id: "chat::send" }, async ({ roomId, message }) => {
  const logger = new Logger()

  await iii.trigger({ function_id: "stream::set", payload: {
    stream_name: "chat", group_id: roomId,
    item_id: crypto.randomUUID(), data: message
  } })

  const history = await iii.trigger({ function_id: "stream::list", payload: {
    stream_name: "chat", group_id: roomId
  } })

  if (history.length > 100) {
    const summary = await iii.trigger({ function_id: "agents::summarize", payload: { history } })
    await iii.trigger({ function_id: "state::set", payload: {
      scope: roomId, key: "summary", value: summary
    } })
  }
  logger.info("Message sent", { roomId, messages: history.length })
})

iii.onFunctionsAvailable((fns) => {
  logger.info("System topology changed", { count: fns.length })
})`,
      python: `async def send_message(input):
    logger = Logger()

    await iii.trigger({"function_id": "stream::set", "payload": {
        "stream_name": "chat", "group_id": input["roomId"],
        "item_id": str(uuid4()), "data": input["message"],
    }})

    history = await iii.trigger({"function_id": "stream::list", "payload": {
        "stream_name": "chat", "group_id": input["roomId"],
    }})

    if len(history) > 100:
        summary = await iii.trigger({"function_id": "agents::summarize", "payload": {"history": history}})
        await iii.trigger({"function_id": "state::set", "payload": {
            "scope": input["roomId"], "key": "summary", "value": summary,
        }})
    logger.info("Message sent", room=input["roomId"], count=len(history))

iii.register_function("chat::send", send_message)

iii.on_functions_available(
    lambda fns: Logger().info("Topology changed", count=len(fns))
)`,
      rust: `use iii_sdk::TriggerRequest;

iii.register_function(
    reg("chat::send"), |input| async move {
        let logger = Logger::new();
        let room_id = &input.room_id;

        iii.trigger(TriggerRequest::new("stream::set", json!({
            "stream_name": "chat", "group_id": room_id,
            "item_id": Uuid::new_v4().to_string(), "data": input.message,
        }))).await?;

        let history: Vec<Message> = iii.trigger(TriggerRequest::new("stream::list", json!({
            "stream_name": "chat", "group_id": room_id,
        }))).await?;

        if history.len() > 100 {
            let summary = iii.trigger(TriggerRequest::new("agents::summarize", json!({"history": history}))).await?;
            iii.trigger(TriggerRequest::new("state::set", json!({
                "scope": room_id, "key": "summary", "value": summary,
            }))).await?;
        }
        logger.info("Message sent", &format!("{}: {}", room_id, history.len()));
        Ok(json!({"messages": history.len()}))
    },
)?;

iii.on_functions_available(|fns| {
    logger.info("Topology changed", fns.len());
});`,
    },
  },
  {
    name: "Deep Research Agent",
    code: {
      typescript: `iii.registerFunction({ id: "research::deep" }, async ({ question, depth = 3 }) => {
  const logger = new Logger()
  let context: string[] = []

  for (let i = 0; i < depth; i++) {
    const subQueries = await callLLM("Break this into sub-questions", { question, context })
    const results = await Promise.all(
      subQueries.map((q: string) => iii.trigger({ function_id: "tools::webSearch", payload: { query: q } }))
    )
    context.push(...results.flat())

    const assessment = await callLLM("Is this enough to answer?", { question, context })
    if (assessment.sufficient) break
    logger.info("Research iteration", { iteration: i + 1, sources: context.length })
  }

  const report = await callLLM("Write a comprehensive answer", { question, context })
  await iii.trigger({ function_id: "state::set", payload: { scope: "research", key: question, value: report } })
  return report
})`,
      python: `async def deep_research(input):
    logger = Logger()
    context = []

    for i in range(input.get("depth", 3)):
        sub_queries = await call_llm("Break this into sub-questions", {
            "question": input["question"], "context": context,
        })
        results = await asyncio.gather(*[
            iii.trigger({"function_id": "tools::webSearch", "payload": {"query": q}}) for q in sub_queries
        ])
        context.extend([r for batch in results for r in batch])

        assessment = await call_llm("Is this enough?", {
            "question": input["question"], "context": context,
        })
        if assessment["sufficient"]:
            break
        logger.info("Research iteration", iteration=i + 1, sources=len(context))

    report = await call_llm("Write a comprehensive answer", {
        "question": input["question"], "context": context,
    })
    await iii.trigger({"function_id": "state::set", "payload": {
        "scope": "research", "key": input["question"], "value": report,
    }})
    return report

iii.register_function("research::deep", deep_research)`,
      rust: `use iii_sdk::TriggerRequest;

iii.register_function(
    reg("research::deep"), |input| async move {
        let logger = Logger::new();
        let mut context: Vec<String> = vec![];

        for i in 0..input.depth.unwrap_or(3) {
            let sub_queries = call_llm("Break into sub-questions", json!({
                "question": input.question, "context": context,
            })).await?;

            let results = futures::future::join_all(
                sub_queries.iter().map(|q| iii.trigger(TriggerRequest::new("tools::webSearch", json!({"query": q}))))
            ).await;
            context.extend(results.into_iter().flatten());

            let assessment = call_llm("Is this enough?", json!({
                "question": input.question, "context": context,
            })).await?;
            if assessment.sufficient { break; }
            logger.info("Research iteration", &format!("{}: {}", i + 1, context.len()));
        }

        let report = call_llm("Write comprehensive answer", json!({
            "question": input.question, "context": context,
        })).await?;
        iii.trigger(TriggerRequest::new("state::set", json!({
            "scope": "research", "key": input.question, "value": report,
        }))).await?;
        Ok(report)
    },
)?;`,
    },
  },
  {
    name: "Event-Driven Pipelines",
    code: {
      typescript: `import { TriggerAction } from "iii-sdk"

iii.registerFunction({ id: "pipeline::onUserCreated" }, async ({ user }) => {
  const logger = new Logger()

  await Promise.all([
    iii.trigger({ function_id: "crm::syncContact", payload: { user } }),
    iii.trigger({ function_id: "analytics::track", payload: { event: "signup", user } }),
    iii.trigger({ function_id: "ml::computeSegment", payload: { user } }),
  ])

  const segment = await iii.trigger({ function_id: "state::get", payload: {
    scope: user.id, key: "segment"
  } })

  await iii.trigger({
    function_id: "emails::send",
    payload: { template: segment === "enterprise" ? "white-glove" : "welcome", user },
    action: TriggerAction.Enqueue({ queue: "emails" })
  })
  logger.info("Pipeline complete", { userId: user.id, segment })
})

iii.registerTrigger({
  type: "subscribe", function_id: "pipeline::onUserCreated",
  config: { topic: "user.created" }
})`,
      python: `from iii import TriggerAction

async def on_user_created(input):
    logger = Logger()
    user = input["user"]

    await asyncio.gather(
        iii.trigger({"function_id": "crm::syncContact", "payload": {"user": user}}),
        iii.trigger({"function_id": "analytics::track", "payload": {"event": "signup", "user": user}}),
        iii.trigger({"function_id": "ml::computeSegment", "payload": {"user": user}}),
    )

    segment = await iii.trigger({"function_id": "state::get", "payload": {
        "scope": user["id"], "key": "segment",
    }})

    await iii.trigger({
        "function_id": "emails::send",
        "payload": {
            "template": "white-glove" if segment == "enterprise" else "welcome",
            "user": user,
        },
        "action": TriggerAction.Enqueue(queue="emails"),
    })
    logger.info("Pipeline complete", user_id=user["id"], segment=segment)

iii.register_function("pipeline::onUserCreated", on_user_created)

iii.register_trigger("subscribe", "pipeline::onUserCreated", {"topic": "user.created"})`,
      rust: `use iii_sdk::{TriggerRequest, TriggerAction};

iii.register_function(
    reg("pipeline::onUserCreated"), |input| async move {
        let logger = Logger::new();
        let user = &input.user;

        futures::future::join_all(vec![
            iii.trigger(TriggerRequest::new("crm::syncContact", json!({"user": user}))),
            iii.trigger(TriggerRequest::new("analytics::track", json!({"event": "signup", "user": user}))),
            iii.trigger(TriggerRequest::new("ml::computeSegment", json!({"user": user}))),
        ]).await;

        let segment: String = iii.trigger(TriggerRequest::new("state::get", json!({
            "scope": user.id, "key": "segment",
        }))).await?;

        let template = if segment == "enterprise" { "white-glove" } else { "welcome" };
        iii.trigger(TriggerRequest::new("emails::send", json!({
            "template": template, "user": user,
        })).action(TriggerAction::enqueue("emails"))).await?;
        logger.info("Pipeline complete", &format!("{}: {}", user.id, segment));
        Ok(json!({"status": "ok"}))
    },
)?;

iii.register_trigger(
    "pipeline::onUserCreated",
    "subscribe",
    json!({"topic": "user.created"}),
);`,
    },
  },
  {
    name: "Scheduled Intelligence",
    code: {
      typescript: `iii.registerFunction({ id: "monitor::anomalies" }, async () => {
  const logger = new Logger()

  const metrics = await iii.trigger({ function_id: "metrics::getLast24h", payload: {} })
  const baseline = await iii.trigger({ function_id: "state::get", payload: {
    scope: "monitor", key: "baseline"
  } })

  const analysis = await callLLM(
    "Analyze these metrics against baseline. Flag anomalies.", { metrics, baseline }
  )

  if (analysis.anomalies.length > 0) {
    await iii.trigger({ function_id: "alerts::send", payload: {
      channel: "slack", message: analysis.summary,
      severity: analysis.anomalies[0].severity
    } })
    logger.info("Anomalies detected", { count: analysis.anomalies.length })
  }

  await iii.trigger({ function_id: "state::set", payload: {
    scope: "monitor", key: "baseline",
    value: { ...baseline, ...metrics.averages }
  } })
})

iii.registerTrigger({
  type: "cron", function_id: "monitor::anomalies",
  config: { pattern: "*/15 * * * *" }
})`,
      python: `async def detect_anomalies(input):
    logger = Logger()

    metrics = await iii.trigger({"function_id": "metrics::getLast24h", "payload": {}})
    baseline = await iii.trigger({"function_id": "state::get", "payload": {
        "scope": "monitor", "key": "baseline",
    }})

    analysis = await call_llm(
        "Analyze metrics against baseline. Flag anomalies.",
        metrics=metrics, baseline=baseline,
    )

    if analysis["anomalies"]:
        await iii.trigger({"function_id": "alerts::send", "payload": {
            "channel": "slack", "message": analysis["summary"],
            "severity": analysis["anomalies"][0]["severity"],
        }})
        logger.info("Anomalies detected", count=len(analysis["anomalies"]))

    await iii.trigger({"function_id": "state::set", "payload": {
        "scope": "monitor", "key": "baseline",
        "value": {**baseline, **metrics["averages"]},
    }})

iii.register_function("monitor::anomalies", detect_anomalies)

iii.register_trigger("cron", "monitor::anomalies", {"pattern": "*/15 * * * *"})`,
      rust: `use iii_sdk::TriggerRequest;

iii.register_function(
    reg("monitor::anomalies"), |_| async move {
        let logger = Logger::new();

        let metrics = iii.trigger(TriggerRequest::new("metrics::getLast24h", json!({}))).await?;
        let baseline = iii.trigger(TriggerRequest::new("state::get", json!({
            "scope": "monitor", "key": "baseline",
        }))).await?;

        let analysis = call_llm(
            "Analyze metrics against baseline. Flag anomalies.",
            json!({"metrics": metrics, "baseline": baseline}),
        ).await?;

        if !analysis.anomalies.is_empty() {
            iii.trigger(TriggerRequest::new("alerts::send", json!({
                "channel": "slack", "message": analysis.summary,
                "severity": analysis.anomalies[0].severity,
            }))).await?;
            logger.info("Anomalies detected", analysis.anomalies.len());
        }

        iii.trigger(TriggerRequest::new("state::set", json!({
            "scope": "monitor", "key": "baseline",
            "value": merge_json(&baseline, &metrics.averages),
        }))).await?;
        Ok(json!({"checked": true}))
    },
)?;

iii.register_trigger(
    "monitor::anomalies",
    "cron",
    json!({"pattern": "*/15 * * * *"}),
);`,
    },
  },
];

const ClaudeCodeIcon = () => (
  <svg
    viewBox="25 25 50 50"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path d="m39.0623 55.9433 7.0836-3.9744.1191-.3451-.1191-.192-.3452-.0001-1.1838-.073-4.0473-.1094-3.5101-.1458-3.4008-.1824-.8556-.1822-.8021-1.0574.0826-.5274.7196-.4838 1.0307.09 2.2776.1555 3.4177.2358 2.4795.1459 3.6731.3816h.5832l.0827-.2357-.1994-.1459-.1556-.1459-3.5368-2.3968-3.8286-2.533-2.0054-1.4585-1.0842-.7389-.5469-.6929-.2357-1.5119.9845-1.0841 1.3224.0899.3378.09 1.3395 1.0306 2.861 2.2145 3.7361 2.7517.5469.4546.2188-.1555.0268-.1094-.2456-.4108-2.0321-3.6731-2.1683-3.7361-.965-1.5484-.2553-.9286c-.0899-.3816-.1555-.7024-.1555-1.0938l1.1206-1.5218.6198-.1993 1.495.1993.6297.547.9285 2.1246 1.5047 3.3449 2.3336 4.548.6831 1.3491.3646 1.2495.1362.3816.2355-.0001v-.2188l.1921-2.5621.3549-3.1455.3451-4.0473.1192-1.14.5638-1.3663 1.1206-.7388.8751.4181.7196 1.0306-.0997.6661-.4278 2.7808-.8387 4.356-.5469 2.917h.3186l.3647-.3646 1.4755-1.9592 2.4795-3.0993 1.0939-1.23 1.2762-1.3588.819-.6467h1.5484l1.14 1.6942-.5103 1.7503-1.5947 2.0224-1.3223 1.7136-1.8962 2.5525-1.1837 2.0418.1094.1629.282-.0267 4.2831-.9117 2.3141-.418 2.7615-.474 1.2494.5834.1362.5931-.491 1.213-2.9535.7293-3.4639.6928-5.1583 1.2204-.0632.0461.0729.0899 2.3239.2188.9942.0535h2.4332l4.5311.338 1.1837.7827.7099.9578-.1191.7293-1.8231.9285-2.46-.5834-5.7417-1.3661-1.969-.4911-.2722-.0001v.163l1.6408 1.6043 3.0069 2.7152 3.7654 3.5004.192.8655-.4837.683-.5104-.0729-3.3084-2.4893-1.2762-1.1205-2.8903-2.4333-.1919-.0001v.2552l.6661.9748 3.5174 5.2871.1822 1.6214-.2551.5275-.9117.3184-1.0015-.1822-2.0588-2.8904-2.1247-3.2549-1.7137-2.9169-.209.119-1.0112 10.8926-.474.5566-1.0939.4182-.9115-.6929-.4838-1.1206.4838-2.2145.5834-2.8902.4739-2.2971.4279-2.8539.2552-.948-.017-.0632-.209.0267-2.1514 2.9535-3.2718 4.4217-2.5889 2.7712-.6198.2455-1.0745-.5567.0997-.9942.6004-.8847 3.5831-4.5578 2.161-2.8247 1.3952-1.6312-.0097-.2357h-.0826l-9.5166 6.1792-1.6944.2187-.7293-.6831.0901-1.1206.3451-.3645 2.8611-1.9691-.0098.0098z" />
  </svg>
);

const CursorIcon = () => (
  <svg
    viewBox="30 28 40 42"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path d="m64.4023 41.0481-13.6917-7.8583c-.4396-.2524-.9821-.2524-1.4218 0l-13.691 7.8583c-.3696.2121-.5977.6045-.5977 1.0294v15.8463c0 .4249.2281.8173.5977 1.0294l13.6917 7.8583c.4396.2524.9821.2524 1.4218 0l13.6916-7.8583c.3696-.2121.5978-.6045.5978-1.0294v-15.8463c0-.4249-.2282-.8173-.5978-1.0294zm-.86 1.6646-13.2173 22.7582c-.0894.1534-.3253.0907-.3253-.0869v-14.9019c0-.2977-.16-.5731-.4197-.7227l-12.9814-7.4506c-.1542-.0888-.0912-.3233.0874-.3233h26.4346c.3754 0 .61.4045.4223.7278h-.0006z" />
  </svg>
);

const GeminiIcon = () => (
  <svg
    viewBox="28 28 44 44"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path d="m48.56 60.9807c.96 2.19 1.44 4.53 1.44 7.02 0-2.49.465-4.83 1.395-7.02.96-2.19 2.25-4.095 3.87-5.715s3.525-2.895 5.715-3.825c2.19-.96 4.53-1.44 7.02-1.44-2.49 0-4.83-.465-7.02-1.395-2.1315-.919-4.0704-2.232-5.715-3.87-1.638-1.6446-2.951-3.5835-3.87-5.715-.93-2.19-1.395-4.53-1.395-7.02 0 2.49-.48 4.83-1.44 7.02-.93 2.19-2.205 4.095-3.825 5.715-1.6446 1.638-3.5835 2.951-5.715 3.87-2.19.93-4.53 1.395-7.02 1.395 2.49 0 4.83.48 7.02 1.44 2.19.93 4.095 2.205 5.715 3.825s2.895 3.525 3.825 5.715z" />
  </svg>
);

const CodexIcon = () => (
  <svg
    viewBox="28 28 44 44"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path d="m65.6261 46.7346c.8277-2.4514.5427-5.1369-.781-7.3667-1.9907-3.42-5.9926-5.1796-9.9009-4.3515-1.7388-1.9328-4.2368-3.032-6.8557-3.0162-3.995-.009-7.5397 2.529-8.7688 6.2799-2.5664.5186-4.7817 2.1037-6.0781 4.3504-2.0055 3.4111-1.5483 7.7109 1.131 10.636-.8277 2.4514-.5427 5.1369.781 7.3667 1.9907 3.42 5.9926 5.1796 9.901 4.3515 1.7376 1.9328 4.2367 3.032 6.8556 3.0151 3.9974.0101 7.5432-2.5302 8.7723-6.2844 2.5664-.5186 4.7817-2.1038 6.078-4.3504 2.0033-3.4111 1.5449-7.7076-1.1333-10.6326zm-13.7136 18.9128c-1.5996.0022-3.149-.5501-4.377-1.5615.0559-.0293.1528-.0822.2155-.1204l7.265-4.1401c.3717-.2081.5997-.5985.5974-1.0204v-10.1061l3.0704 1.7494c.0331.0158.0547.0473.0593.0833v8.369c-.0046 3.7216-3.059 6.7389-6.8306 6.7468zm-14.6895-6.191c-.8015-1.3658-1.09-2.9667-.8152-4.5204.0536.0315.1482.0889.2155.1272l7.2649 4.14c.3683.2127.8244.2127 1.1938 0l8.8691-5.0536v3.4989c.0023.036-.0148.0708-.0433.0933l-7.3437 4.184c-3.271 1.8585-7.4485.7538-9.34-2.4694zm-1.912-15.648c.7981-1.368 2.0579-2.4143 3.5584-2.9577 0 .0619-.0035.171-.0035.2475v8.2813c-.0022.4208.2258.8112.5963 1.0193l8.8692 5.0525-3.0704 1.7494c-.0308.0203-.0696.0236-.1038.009l-7.3448-4.1873c-3.2642-1.8653-4.3838-5.9863-2.5026-9.2129zm25.2267 5.7928-8.8692-5.0536 3.0704-1.7483c.0308-.0203.0696-.0237.1038-.009l7.3447 4.1839c3.27 1.8642 4.3907 5.9919 2.5015 9.2185-.7992 1.3658-2.0579 2.412-3.5572 2.9565v-8.5288c.0034-.4207-.2235-.81-.5929-1.0192zm3.0555-4.5384c-.0536-.0326-.1482-.0889-.2155-.1271l-7.2649-4.1401c-.3683-.2126-.8243-.2126-1.1937 0l-8.8692 5.0536v-3.4988c-.0022-.036.0149-.0709.0434-.0934l7.3436-4.1806c3.271-1.8619 7.4531-.7537 9.3389 2.4751.7969 1.3635 1.0854 2.9599.8152 4.5113zm-19.2124 6.236-3.0716-1.7494c-.033-.0157-.0547-.0472-.0592-.0832v-8.3691c.0022-3.7261 3.0658-6.7456 6.8419-6.7434 1.5974 0 3.1434.5535 4.3713 1.5616-.0559.0292-.1516.0821-.2155.1203l-7.2649 4.1401c-.3717.2081-.5997.5974-.5975 1.0193l-.0045 10.1016zm1.668-3.5483 3.9506-2.2512 3.9506 2.2501v4.5012l-3.9506 2.25-3.9506-2.25z" />
  </svg>
);

const WindsurfIcon = () => (
  <svg
    viewBox="22 30 56 40"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path d="m73.6959 36.1347h-.4574c-2.4075-.0037-4.3611 1.9459-4.3611 4.353v9.7352c0 1.944-1.6069 3.5187-3.5193 3.5187-1.1362 0-2.2705-.5718-2.9436-1.5316l-9.9428-14.2006c-.8249-1.1792-2.1673-1.8822-3.6204-1.8822-2.2668 0-4.3067 1.9271-4.3067 4.3061v9.7914c0 1.9441-1.5937 3.5188-3.5192 3.5188-1.14 0-2.2725-.5718-2.9456-1.5316l-11.1258-15.8916c-.2512-.36-.8156-.1818-.8156.2569v8.4903c0 .4293.1313.8455.3769 1.1979l10.9495 15.6367c.6469.9242 1.6013 1.6103 2.7018 1.8597 2.7543.6261 5.2892-1.4942 5.2892-4.1956v-9.7858c0-1.944 1.5749-3.5188 3.5192-3.5188h.0056c1.1719 0 2.2706.5718 2.9437 1.5317l9.9446 14.1988c.8268 1.181 2.0999 1.8821 3.6186 1.8821 2.3174 0 4.303-1.929 4.303-4.3061v-9.7895c0-1.944 1.5749-3.5188 3.5192-3.5188h.3881c.2437 0 .4406-.1969.4406-.4405v-9.2441c0-.2436-.1969-.4405-.4406-.4405z" />
  </svg>
);

const TraeIcon = () => (
  <svg
    viewBox="30 35 40 32"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path
      clipRule="evenodd"
      d="m67 63.1005h-29.1437v-4.8535h-4.8563v-19.4296h34zm-29.1437-4.8535h24.2874v-14.5746h-24.2874zm12.1451-7.361-3.4354 3.434-3.434-3.434 3.434-3.434zm9.7141-.0014-3.434 3.4326-3.4354-3.4326 3.4354-3.4354z"
      fillRule="evenodd"
    />
  </svg>
);

const AmpIcon = () => (
  <svg
    viewBox="28 28 44 44"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path d="m37.1786 64.235 9.0088-9.1362 3.2785 12.4474 4.7627-1.3025-4.7452-18.0735-17.7865-4.8172-1.2665 4.8664 12.2388 3.3247-8.9713 9.1227z" />
    <path d="m63.157 53.6443 4.7627-1.3026-4.745-18.0734-17.7866-4.8173-1.2665 4.8665 15.0198 4.0801z" />
    <path d="m56.3208 60.5908 4.7626-1.3026-4.7451-18.0734-17.7865-4.8172-1.2665 4.8664 15.0197 4.0801z" />
  </svg>
);

const RooIcon = () => (
  <svg
    viewBox="25 30 50 40"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path d="m64.2039 38.0382-.7929 2.86c-.0413.1513-.2017.2384-.353.1925l-12.8837-3.992c-.0871-.0275-.1879-.0092-.2567.0504l-13.2687 10.6425c-.0367.0321-.0825.0504-.133.0596l-7.9062 1.2191c-.1421.0229-.2429.1467-.2383.2888l.0504 1.1321c.0046.142.1191.2566.2612.2658l9.2034.5637c.0504 0 .1008-.0091.142-.0275l6.71-3.3916c.0963-.0504.2109-.0367.2934.0275l4.2946 3.2221c.0733.055.1145.1375.11.2245l-.0413 5.3396c0 .0596.0183.1146.0504.1604l6.7559 9.6938c.0504.0733.1375.1192.2291.1192h2.3054c.2109 0 .3484-.2292.2475-.4125l-4.982-9.13c-.0459-.0825-.0459-.1834 0-.2659l2.5254-4.8033c.0275-.0504.0687-.0917.1191-.1192l8.965-4.5466c.0917-.0459.1971-.0413.2842.0183l2.5667 1.7096c.0458.0321.1008.0458.1558.0458h2.3604c.22 0 .3529-.2383.2384-.4262l-6.5084-10.78c-.1283-.2109-.4445-.165-.5087.0687z" />
  </svg>
);

const CopilotIcon = () => (
  <svg
    viewBox="-3 -3 30 30"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
  >
    <path d="M23.922 16.997C23.061 18.492 18.063 22.02 12 22.02 5.937 22.02.939 18.492.078 16.997A.641.641 0 0 1 0 16.741v-2.869a.883.883 0 0 1 .053-.22c.372-.935 1.347-2.292 2.605-2.656.167-.429.414-1.055.644-1.517a10.098 10.098 0 0 1-.052-1.086c0-1.331.282-2.499 1.132-3.368.397-.406.89-.717 1.474-.952C7.255 2.937 9.248 1.98 11.978 1.98c2.731 0 4.767.957 6.166 2.093.584.235 1.077.546 1.474.952.85.869 1.132 2.037 1.132 3.368 0 .368-.014.733-.052 1.086.23.462.477 1.088.644 1.517 1.258.364 2.233 1.721 2.605 2.656a.841.841 0 0 1 .053.22v2.869a.641.641 0 0 1-.078.256Zm-11.75-5.992h-.344a4.359 4.359 0 0 1-.355.508c-.77.947-1.918 1.492-3.508 1.492-1.725 0-2.989-.359-3.782-1.259a2.137 2.137 0 0 1-.085-.104L4 11.746v6.585c1.435.779 4.514 2.179 8 2.179 3.486 0 6.565-1.4 8-2.179v-6.585l-.098-.104s-.033.045-.085.104c-.793.9-2.057 1.259-3.782 1.259-1.59 0-2.738-.545-3.508-1.492a4.359 4.359 0 0 1-.355-.508Zm2.328 3.25c.549 0 1 .451 1 1v2c0 .549-.451 1-1 1-.549 0-1-.451-1-1v-2c0-.549.451-1 1-1Zm-5 0c.549 0 1 .451 1 1v2c0 .549-.451 1-1 1-.549 0-1-.451-1-1v-2c0-.549.451-1 1-1Zm3.313-6.185c.136 1.057.403 1.913.878 2.497.442.544 1.134.938 2.344.938 1.573 0 2.292-.337 2.657-.751.384-.435.558-1.15.558-2.361 0-1.14-.243-1.847-.705-2.319-.477-.488-1.319-.862-2.824-1.025-1.487-.161-2.192.138-2.533.529-.269.307-.437.808-.438 1.578v.021c0 .265.021.562.063.893Zm-1.626 0c.042-.331.063-.628.063-.894v-.02c-.001-.77-.169-1.271-.438-1.578-.341-.391-1.046-.69-2.533-.529-1.505.163-2.347.537-2.824 1.025-.462.472-.705 1.179-.705 2.319 0 1.211.175 1.926.558 2.361.365.414 1.084.751 2.657.751 1.21 0 1.902-.394 2.344-.938.475-.584.742-1.44.878-2.497Z" />
  </svg>
);

const ClineIcon = () => (
  <svg
    viewBox="-3 -3 30 30"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
    fillRule="evenodd"
  >
    <path d="M17.035 3.991c2.75 0 4.98 2.24 4.98 5.003v1.667l1.45 2.896a1.01 1.01 0 01-.002.909l-1.448 2.864v1.668c0 2.762-2.23 5.002-4.98 5.002H7.074c-2.751 0-4.98-2.24-4.98-5.002V17.33l-1.48-2.855a1.01 1.01 0 01-.003-.927l1.482-2.887V8.994c0-2.763 2.23-5.003 4.98-5.003h9.962zM8.265 9.6a2.274 2.274 0 00-2.274 2.274v4.042a2.274 2.274 0 004.547 0v-4.042A2.274 2.274 0 008.265 9.6zm7.326 0a2.274 2.274 0 00-2.274 2.274v4.042a2.274 2.274 0 104.548 0v-4.042A2.274 2.274 0 0015.59 9.6z" />
    <path d="M12.054 5.558a2.779 2.779 0 100-5.558 2.779 2.779 0 000 5.558z" />
  </svg>
);

const GooseIcon = () => (
  <svg
    viewBox="-3 -3 30 30"
    className="w-8 h-8 md:w-10 md:h-10"
    fill="currentColor"
    fillRule="evenodd"
  >
    <path d="M21.595 23.61c1.167-.254 2.405-.944 2.405-.944l-2.167-1.784a12.124 12.124 0 01-2.695-3.131 12.127 12.127 0 00-3.97-4.049l-.794-.462a1.115 1.115 0 01-.488-.815.844.844 0 01.154-.575c.413-.582 2.548-3.115 2.94-3.44.503-.416 1.065-.762 1.586-1.159.074-.056.148-.112.221-.17.003-.002.007-.004.009-.007.167-.131.325-.272.45-.438.453-.524.563-.988.59-1.193-.061-.197-.244-.639-.753-1.148.319.02.705.272 1.056.569.235-.376.481-.773.727-1.171.165-.266-.08-.465-.086-.471h-.001V3.22c-.007-.007-.206-.25-.471-.086-.567.35-1.134.702-1.639 1.021 0 0-.597-.012-1.305.599a2.464 2.464 0 00-.438.45l-.007.009c-.058.072-.114.147-.17.221-.397.521-.743 1.083-1.16 1.587-.323.391-2.857 2.526-3.44 2.94a.842.842 0 01-.574.153 1.115 1.115 0 01-.815-.488l-.462-.794a12.123 12.123 0 00-4.049-3.97 12.133 12.133 0 01-3.13-2.695L1.332 0S.643 1.238.39 2.405c.352.428 1.27 1.49 2.34 2.302C1.58 4.167.73 3.75.06 3.4c-.103.765-.063 1.92.043 2.816.726.317 1.961.806 3.219 1.066-1.006.236-2.11.278-2.961.262.15.554.358 1.119.64 1.688.119.263.25.52.39.77.452.125 2.222.383 3.164.171l-2.51.897a27.776 27.776 0 002.544 2.726c2.031-1.092 2.494-1.241 4.018-2.238-2.467 2.008-3.108 2.828-3.8 3.67l-.483.678c-.25.351-.469.725-.65 1.117-.61 1.31-1.47 4.1-1.47 4.1-.154.486.202.842.674.674 0 0 2.79-.861 4.1-1.47.392-.182.766-.4 1.118-.65l.677-.483c.227-.187.453-.37.701-.586 0 0 1.705 2.02 3.458 3.349l.896-2.511c-.211.942.046 2.712.17 3.163.252.142.509.272.772.392.569.28 1.134.49 1.688.64-.016-.853.026-1.956.261-2.962.26 1.258.75 2.493 1.067 3.219.895.106 2.051.146 2.816.043a73.87 73.87 0 01-1.308-2.67c.811 1.07 1.874 1.988 2.302 2.34h-.001z" />
  </svg>
);

const agentLogos = [
  { name: "Claude Code", icon: ClaudeCodeIcon },
  { name: "Cursor", icon: CursorIcon },
  { name: "Gemini", icon: GeminiIcon },
  { name: "Codex", icon: CodexIcon },
  { name: "Windsurf", icon: WindsurfIcon },
  { name: "Trae", icon: TraeIcon },
  { name: "Amp", icon: AmpIcon },
  { name: "Roo", icon: RooIcon },
  { name: "Copilot", icon: CopilotIcon },
  { name: "Cline", icon: ClineIcon },
  { name: "Goose", icon: GooseIcon },
];

export function AgentReadySection({
  isDarkMode = true,
}: AgentReadySectionProps) {
  const [activeTab, setActiveTab] = useState(0);
  const [activeLang, setActiveLang] = useState<Lang>("typescript");
  const [isHovered, setIsHovered] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);

  const textPrimary = isDarkMode ? "text-iii-light" : "text-iii-black";
  const textSecondary = isDarkMode ? "text-iii-light/70" : "text-iii-black/70";
  const infoColor = isDarkMode ? "text-iii-info" : "text-[#0891b2]";
  const iconBase = isDarkMode
    ? "text-iii-medium-dark"
    : "text-iii-medium-light";
  const cardBg = isDarkMode ? "bg-iii-dark/30" : "bg-white/50";
  const cardBorder = isDarkMode ? "border-iii-light/15" : "border-iii-black/15";

  useEffect(() => {
    if (isHovered) return;
    const interval = setInterval(() => {
      setActiveTab((prev) => (prev + 1) % capabilities.length);
    }, 5000);
    return () => clearInterval(interval);
  }, [isHovered]);

  const copyToClipboard = useCallback(() => {
    navigator.clipboard
      .writeText(capabilities[activeTab].code[activeLang])
      .then(() => {
        setCopySuccess(true);
        setTimeout(() => setCopySuccess(false), 2000);
      });
  }, [activeTab, activeLang]);

  const active = capabilities[activeTab];

  return (
    <section
      className={`relative overflow-hidden font-mono transition-colors duration-300 ${textPrimary}`}
    >
      <style>{`
        @keyframes agent-scroll {
          0% { transform: translateX(0); }
          100% { transform: translateX(-25%); }
        }
      `}</style>
      <div className="absolute inset-0 pointer-events-none overflow-hidden">
        <div
          className="absolute -top-1/3 -left-1/4 w-1/2 h-1/2 rounded-full opacity-[0.025]"
          style={{
            background:
              "radial-gradient(circle, var(--color-info) 0%, transparent 70%)",
          }}
        />
        <div
          className="absolute -bottom-1/4 -right-1/4 w-2/5 h-2/5 rounded-full opacity-[0.02]"
          style={{
            background:
              "radial-gradient(circle, var(--color-accent) 0%, transparent 70%)",
          }}
        />
      </div>
      <div className="relative z-10">
        <div className="text-center mb-10 md:mb-16 space-y-4">
          <div className="flex items-center justify-center gap-2 mb-4">
            <BrainCircuitIcon size={20} className={infoColor} />
            <span
              className={`text-xs md:text-sm font-mono tracking-wider uppercase ${infoColor}`}
            >
              Agent-Ready
            </span>
          </div>
          <h2 className="text-xl sm:text-2xl md:text-3xl lg:text-4xl xl:text-5xl font-bold tracking-tighter">
            AI agents are
            <br />
            <span className={infoColor}>first-class citizens</span>
          </h2>
          <p
            className={`text-sm md:text-base lg:text-lg max-w-3xl mx-auto ${textSecondary}`}
          >
            The engine operates as a universal tool discovery and invocation
            layer where intelligent agents participate as first-class execution
            entities — built in from day one.
          </p>
        </div>

        <div className="mb-10 md:mb-14 overflow-hidden">
          <p
            className={`text-center text-xs uppercase tracking-widest mb-6 ${textSecondary}`}
          >
            The universal agentic layer
          </p>
          <div
            className="flex w-max"
            style={{ animation: "agent-scroll 60s linear infinite" }}
          >
            {[...agentLogos, ...agentLogos, ...agentLogos, ...agentLogos].map(
              (logo, i) => {
                const Icon = logo.icon;
                return (
                  <div
                    key={i}
                    className="flex flex-col items-center gap-2 px-4 md:px-6 shrink-0"
                  >
                    <div className={iconBase}>
                      <Icon />
                    </div>
                    <span
                      className={`text-[10px] whitespace-nowrap ${textPrimary}`}
                    >
                      {logo.name}
                    </span>
                  </div>
                );
              },
            )}
          </div>
        </div>

        <div
          className={`rounded-lg border overflow-hidden ${cardBg} ${cardBorder}`}
          onMouseEnter={() => setIsHovered(true)}
          onMouseLeave={() => setIsHovered(false)}
        >
          <div className="flex md:hidden flex-wrap gap-2 p-3">
            {capabilities.map((cap, i) => (
              <button
                key={cap.name}
                onClick={() => setActiveTab(i)}
                className={`px-3 py-1.5 rounded-full text-[10px] whitespace-nowrap border transition-all ${
                  activeTab === i
                    ? isDarkMode
                      ? "bg-iii-info/15 border-iii-info/40 text-iii-info"
                      : "bg-[#0891b2]/10 border-[#0891b2]/40 text-[#0891b2]"
                    : isDarkMode
                      ? "border-iii-light/10 text-iii-light/50 hover:border-iii-light/25"
                      : "border-iii-black/10 text-iii-black/50 hover:border-iii-black/25"
                }`}
              >
                {cap.name}
              </button>
            ))}
          </div>

          <div className="flex flex-col md:flex-row md:h-[480px]">
            <div
              className={`hidden md:flex flex-col w-72 lg:w-80 shrink-0 border-r ${cardBorder}`}
            >
              {capabilities.map((cap, i) => (
                <button
                  key={cap.name}
                  onClick={() => setActiveTab(i)}
                  className={`flex items-center justify-between text-left px-6 py-5 transition-all duration-200 border-b ${cardBorder} ${
                    activeTab === i
                      ? isDarkMode
                        ? "bg-iii-light/10"
                        : "bg-iii-black/8"
                      : isDarkMode
                        ? "hover:bg-iii-light/5"
                        : "hover:bg-iii-black/5"
                  }`}
                >
                  <span
                    className={`text-sm font-medium tracking-tight ${
                      activeTab === i ? textPrimary : textSecondary
                    }`}
                  >
                    {cap.name}
                  </span>
                  <svg
                    viewBox="0 0 24 24"
                    className={`w-4 h-4 shrink-0 ml-3 ${
                      activeTab === i ? infoColor : textSecondary
                    }`}
                    fill="none"
                    stroke="currentColor"
                    strokeWidth={2}
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <path d="m9 18 6-6-6-6" />
                  </svg>
                </button>
              ))}
            </div>

            <div className="flex-1 min-w-0 flex flex-col">
              <div
                className={`flex items-center justify-between px-4 py-2 border-b ${cardBorder}`}
              >
                <div
                  className={`flex items-center gap-0.5 rounded-full p-1 ${
                    isDarkMode ? "bg-white/[0.06]" : "bg-black/[0.05]"
                  }`}
                >
                  {(
                    [
                      {
                        key: "typescript" as const,
                        label: "TypeScript",
                        short: "TS",
                      },
                      { key: "python" as const, label: "Python", short: "PY" },
                      { key: "rust" as const, label: "Rust", short: "RS" },
                    ] as const
                  ).map((lang) => (
                    <button
                      key={lang.key}
                      onClick={() => setActiveLang(lang.key)}
                      className={`flex items-center gap-2 px-3 py-1.5 text-[11px] sm:text-xs font-mono rounded-full transition-all duration-200 ${
                        activeLang === lang.key
                          ? isDarkMode
                            ? "bg-white/[0.12] text-white"
                            : "bg-white text-black shadow-sm"
                          : isDarkMode
                            ? "text-white/40 hover:text-white/60"
                            : "text-black/40 hover:text-black/60"
                      }`}
                    >
                      <LangIcon
                        lang={lang.key}
                        active={activeLang === lang.key}
                      />
                      <span className="hidden sm:inline">{lang.label}</span>
                      <span className="sm:hidden">{lang.short}</span>
                    </button>
                  ))}
                </div>
                <button
                  onClick={copyToClipboard}
                  className={`p-1.5 rounded-lg transition-all ${
                    copySuccess
                      ? infoColor
                      : `${textSecondary} ${isDarkMode ? "hover:text-iii-light" : "hover:text-iii-black"}`
                  }`}
                >
                  {copySuccess ? (
                    <CheckedIcon size={16} />
                  ) : (
                    <CopyIcon size={16} />
                  )}
                </button>
              </div>

              <div className="p-4 sm:p-6 lg:p-8 overflow-auto flex-1 h-[420px]">
                <Highlight
                  key={isDarkMode ? "dark" : "light"}
                  theme={isDarkMode ? themes.nightOwl : themes.github}
                  code={active.code[activeLang]}
                  language={
                    activeLang === "rust"
                      ? "rust"
                      : activeLang === "python"
                        ? "python"
                        : "typescript"
                  }
                >
                  {({ tokens, getLineProps, getTokenProps }) => (
                    <pre className="text-[11px] sm:text-xs md:text-sm leading-6 md:leading-7">
                      {tokens.map((line, i) => (
                        <div key={i} {...getLineProps({ line })}>
                          {line.map((token, key) => (
                            <span key={key} {...getTokenProps({ token })} />
                          ))}
                        </div>
                      ))}
                    </pre>
                  )}
                </Highlight>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
