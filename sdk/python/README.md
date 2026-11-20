# Machina Python SDK

Standard-library-only client for the Machina controller API.

```python
from machina import Client
c = Client("https://10.0.0.5:5093", token="<API key>")
t = c.create_vm("web-1", vcpus=4, memory="8Gi")
c.wait_task(t.task_id)
print(c.find_vm_by_name("web-1"))
```

Run the tests with `python -m unittest discover -s tests`.
