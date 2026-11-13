# Can a personal app search a user's whole Google Photos library today?

Type: research
Status: resolved
Blocked by: none

## Question

"Find my Mount Rainier photo" is a v0.1 success criterion. Google restricted the Photos Library API in 2025 (reportedly apps can only read media they created, and full-library access moved to the user-driven Picker API). Establish from Google's current docs what a self-hosted personal app can do: search/list the user's whole library, content search ("mountain"), location/date filters, OAuth requirements and app verification for personal use. Compare fallbacks: the Picker API, browser automation of photos.google.com in the user's own session, Google Takeout to a local folder, or local photo libraries. Recommend how the Mount Rainier scenario should work, or whether a different first Lucy-written Plugin makes a better success criterion.

## Answer

Since 31 Mar 2025, apps can't list or search a whole Google Photos library through any API. The Library API reads only media the app created itself. With the Picker API the user does the searching and picking in Google's UI, and Lucy gets no location data. It also needs a per-user Cloud project and OAuth client. Scripting photos.google.com goes against Google's Terms, because robots.txt disallows `/search/`.
Recommendation: keep the scenario but make it local. Lucy writes a Plugin that searches the configured photo folders (local library or a scheduled Takeout export) by EXIF GPS near Rainier, by date and by filename. As a cheap extra, it can open `photos.google.com/search/<query>` in the user's browser for Google's own search. The Picker integration waits until after v0.1.
[findings](../research/google-photos-access.md)
