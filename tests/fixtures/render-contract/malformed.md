# Readable enhancement failures

Invalid math should become a readable error node rather than hanging:

$$
\definitely-not-a-katex-command{
$$

Invalid Mermaid should also settle as a readable error node:

```mermaid
flowchart TD
  A --[ broken
```
