# NetworkTables Configuration

In order to connect to NetworkTables, we need an address and an identity. The identity can be any URL-safe string, and the address should be the server address. Configuration goes under the `[ntable]` table. This table is required to be present in order to publish values from components.

## `ntable.identity`

This should be unique per client connecting to the server, and a URL-safe string.

## `ntable.host`

The host can be explicitly specified through this field, in which case the client will try to connect to a server on this port. It's an error to have both this field and `ntable.team` present.

## `ntable.team`

Alternatively, a team number can be set, in which case the client will try to connect to `10.TE.AM.1`, where `TEAM` is the team number. It's an error to have both this field and `ntable.host` present.
