# Machina TypeScript SDK (GA v1)

```bash
cd sdk/typescript && npm install && npm run build
```

```typescript
import { MachinaClient } from '@zyvor/machina-sdk'

const client = new MachinaClient({
  baseUrl: 'http://127.0.0.1:5093',
  username: 'admin',
  password: process.env.MACHINA_CONTROLLER_PASS!,
})

const health = await client.health()
const slos = await client.observabilityOverview()
```

