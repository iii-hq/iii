# AI Agent — Strategic Account Observer
# Subscribes to account.upgraded events, fetches company details
# cross-worker, and runs a simulated LLM analysis.

import asyncio
import json
import os

from iii import register_worker, InitOptions, Logger

iii = register_worker(
    os.environ.get("III_BRIDGE_URL", "ws://localhost:49134"),
    InitOptions(worker_name="ai-agent"),
)
logger = Logger()


def simulate_llm_analysis(company: dict) -> dict:
    """Simulate an LLM analysis of the company.

    To connect a real LLM, replace this function body with an API call:

        import openai
        client = openai.OpenAI()
        response = client.chat.completions.create(
            model="gpt-4",
            messages=[
                {"role": "system", "content": "Analyze this account upgrade..."},
                {"role": "user", "content": json.dumps(company)},
            ],
        )
        return {"analysis": response.choices[0].message.content, ...}
    """
    arr = company.get("arr", 0)
    name = company.get("name", "Unknown")
    plan = company.get("plan", "unknown")

    if arr > 100_000:
        recommendation = "HIGH PRIORITY — schedule executive review"
    elif arr > 50_000:
        recommendation = "MEDIUM PRIORITY — assign account manager"
    else:
        recommendation = "LOW PRIORITY — send automated welcome"

    return {
        "companyId": company.get("companyId"),
        "company_name": name,
        "plan": plan,
        "arr": arr,
        "recommendation": recommendation,
        "analysis": f"{name} upgraded to {plan}. ARR: ${arr:,}. {recommendation}.",
    }


def strategic_observer(event: dict) -> dict:
    """Observe account upgrades and analyze strategic value."""
    company_id = event.get("companyId") or event.get("data", {}).get("companyId")
    logger.info("AI agent received upgrade event", {"companyId": company_id})

    # Cross-worker call: fetch company details from account-events worker
    company = iii.trigger(
        {"function_id": "accounts::get-details", "payload": {"companyId": company_id}}
    )

    if company.get("error"):
        logger.error("Could not fetch company details", {"error": company["error"]})
        return {"analysis": "skipped", "reason": company["error"]}

    # ═══════════════════════════════════════════════════════════════
    # STEP 5: Scope AI agent to high-value accounts only
    # Uncomment the block below to filter out companies with ARR < $50k.
    # This is the "observability fix" — the agent was processing ALL
    # companies, but should only analyze strategic (high-value) accounts.
    # ═══════════════════════════════════════════════════════════════

    # --- UNCOMMENT STEP 5 START ---
    # arr = company.get("arr", 0)
    # if arr < 50_000:
    #     logger.info("Skipping low-value account", {"companyId": company_id, "arr": arr})
    #     print(f'  [AI Agent] Skipped {company.get("name", company_id)} (ARR ${arr:,} < $50k threshold)')
    #     return {"analysis": "skipped", "reason": f"ARR ${arr:,} below $50k threshold"}
    # --- UNCOMMENT STEP 5 END ---

    # Simulated LLM analysis (swap in a real LLM call above)
    analysis = simulate_llm_analysis(company)
    logger.info(
        "AI analysis complete",
        {"companyId": company_id, "recommendation": analysis["recommendation"]},
    )
    print(
        f'  [AI Agent] Analyzed {company.get("name", company_id)}: {analysis["recommendation"]}'
    )

    # ═══════════════════════════════════════════════════════════════
    # STEP 6 (optional): Enrich analysis with legacy system data
    # Uncomment after completing Step 4 (legacy proxy in worker.ts)
    # to pull historical context from the legacy CRM.
    # ═══════════════════════════════════════════════════════════════

    # --- UNCOMMENT STEP 6 START ---
    # try:
    #     legacy_data = iii.trigger(
    #         {"function_id": "legacy::get-status", "payload": {"companyId": company_id}}
    #     )
    #     analysis["legacy_context"] = legacy_data
    #     logger.info("Enriched with legacy data", {"companyId": company_id})
    #     print(f"  [AI Agent] Enriched with legacy CRM data for {company_id}")
    # except Exception as e:
    #     logger.error("Legacy system unavailable", {"error": str(e)})
    # --- UNCOMMENT STEP 6 END ---

    return analysis


iii.register_function({"id": "agents::strategic-observer"}, strategic_observer)

iii.register_trigger(
    {
        "type": "subscribe",
        "function_id": "agents::strategic-observer",
        "config": {"topic": "account.upgraded"},
    }
)

print("✓ Step 3 complete: AI agent is observing account upgrades.")
print(
    "  → Next: Start the legacy-worker (Java), then uncomment Step 4 in workers/account-events/src/worker.ts"
)
print("")

loop = asyncio.new_event_loop()
try:
    loop.run_forever()
except KeyboardInterrupt:
    pass
