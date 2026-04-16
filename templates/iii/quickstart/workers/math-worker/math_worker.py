import os
from iii import register_worker, InitOptions, Logger

iii = register_worker(
    os.environ.get("III_URL", "ws://localhost:49134"),
    InitOptions(worker_name="math-worker"),
)
logger = Logger()


def add_handler(payload: dict) -> dict:
    a = payload.get("a", 0)
    b = payload.get("b", 0)
    logger.info(f"math::add called in Python with a={a}, b={b}")
    result = {"c": a + b}

    # --- Uncomment after: iii worker add iii-state ---
    # running_total = iii.trigger(
    #     {
    #         "function_id": "state::get",
    #         "payload": {"scope": "math", "key": "running_total"},
    #     }
    # )
    # new_total = (running_total or 0) + result["c"]
    # iii.trigger(
    #     {
    #         "function_id": "state::set",
    #         "payload": {"scope": "math", "key": "running_total", "value": new_total},
    #     }
    # )
    # result["running_total"] = new_total

    return result


iii.register_function("math::add", add_handler)

print("Math worker started - listening for calls")
