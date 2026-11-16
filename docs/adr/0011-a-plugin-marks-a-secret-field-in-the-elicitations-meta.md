# A Plugin marks a secret field in the elicitation's `_meta`

A Plugin's mid-call question is an MCP form elicitation. Lucy's one extension to it, the `secret` field, is a string property named after one of the Plugin's declared credentials, listed in the elicitation request's `_meta` under `lucy/secret`:

```json
{"mode": "form", "message": "Your LINEAR_KEY?",
 "requestedSchema": {"type": "object", "properties": {"LINEAR_KEY": {"type": "string"}}},
 "_meta": {"lucy/secret": ["LINEAR_KEY"]}}
```

What is typed goes to the Secret Service; the Plugin gets `"provided"` in that field's place. A dialog that is cancelled or times out comes back as the `cancel` action with `_meta {"lucy/result": "cancelled" | "timed_out"}`, so the Plugin can tell the model which.

## Considered Options

- A `secret` string format or type in the property's schema: `rmcp` parses elicitation schemas into fixed types, so an unknown format fails the whole `input_required` result and an unknown keyword is dropped. A Plugin built on `rmcp` couldn't send one either.
- A naming convention on the property: invisible to a reader of the schema, and it would turn any field with a matching name into a secret.
