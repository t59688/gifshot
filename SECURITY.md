# Security

GifShot is a local screen-capture utility. It intentionally has no network client, cloud upload, account, telemetry, analytics, remote-control, or plugin execution path.

Captured frame bytes are passed only from the Windows capture surface to the in-process GIF encoder. They are not logged. The final GIF is written only to the configured local capture directory and, when enabled, its file path is published to the local Windows clipboard.

Do not attempt to bypass Windows secure-desktop, DRM, protected-content, or display-affinity restrictions. Reports involving unexpected privilege requirements, unsafe path handling, clipboard memory ownership, or capture data leaving the local process should be treated as security issues.
