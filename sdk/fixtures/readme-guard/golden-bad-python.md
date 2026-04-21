# Golden bad (Python)

This fixture MUST make the guard exit 1 — it contains the exact bug the
guard was built to catch: `iii.connect()` is valid Python syntax but the
`iii.III` class does not expose a public `connect` method.

```python
from iii import register_worker

iii = register_worker("ws://localhost:49134")

def greet(data):
    return {"message": f"Hello, {data['name']}!"}

iii.register_function("greet", greet)
iii.connect()  # <-- intentional bug: no such method
```
