# Machina Go SDK

A small typed client for the Machina controller API (machines, hosts, tasks), plus a generic `Do` for anything else.
It is what the [Terraform provider](../../terraform/provider) is built on.

```go
c := machina.New("https://10.0.0.5:5093", os.Getenv("MACHINA_TOKEN")) // API key from Settings → API keys
task, _ := c.CreateVM(ctx, machina.CreateVMRequest{Name: "web-1", VCPUs: 4, Memory: "8Gi"})
_, _ = c.WaitTask(ctx, task.TaskID, 2*time.Second)
vm, _ := c.FindVMByName(ctx, "web-1")
```

`go test ./...` runs the unit tests. Against a lab controller:
`MACHINA_URL=… MACHINA_TOKEN=… go test -tags integration ./machina -v` (set `MACHINA_TEST_CREATE=1` to also build and
delete a throwaway machine; it never touches existing ones).

The controller listens on `:5093` (plain HTTP on localhost by default). To reach it from a workstation use an SSH
tunnel (`ssh -L 15093:127.0.0.1:5093 host`) or put it behind your TLS terminator.
