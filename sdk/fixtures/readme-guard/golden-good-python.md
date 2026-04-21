# Golden good (Python)

This fixture exists so CI can prove the guard is *working* — not silently
letting everything pass. The guard must exit 0 on this file.

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def greet(data):
    return {"message": f"Hello, {data['name']}!"}

iii.register_function("greet", greet)

iii.register_trigger({
    "type": "http",
    "function_id": "greet",
    "config": {"api_path": "/greet", "http_method": "POST"},
})

result = iii.trigger({"function_id": "greet", "payload": {"name": "world"}})
```
