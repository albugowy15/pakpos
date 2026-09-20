# Postman interoperability fixtures

`postman-collection-v2.1.0-schema.json` is the official Postman Collection
v2.1.0 Draft 4 schema, vendored so tests do not depend on network access. It was
retrieved from <https://schema.getpostman.com/json/collection/v2.1.0/collection.json>
on 2026-09-20. Postman's schemas repository is distributed under Apache-2.0;
its license is included in `LICENSE.postman-schemas`.

`representative-v2.1.postman_collection.json` deliberately combines Pakpos's
supported request fields with folders, scripts, variables, authentication, an
unsupported method, and an unsupported body mode. Its loopback port is replaced
by the integration test before requests are sent.
