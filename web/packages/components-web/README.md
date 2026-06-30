# components-web

Reusable Solid components and host/provider contracts for NMP web apps.

Apps should import the stable root API:

```ts
import { NmpComponentHostProvider, NostrAvatar } from "@nmp/components-web";
```

Feature subpaths are also public:

```ts
import { NmpComponentHostProvider } from "@nmp/components-web/component-host";
import { NostrAvatar } from "@nmp/components-web/user-avatar";
```

Split app repos must not add `tsconfig.paths` aliases to `../packages/*` or copy
package source into the app checkout. The NMP registry's raw-source viewer reads
local package source by relative file path; `src/*` is not a public package API.
