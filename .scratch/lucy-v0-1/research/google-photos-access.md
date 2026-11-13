# Can a personal app search a user's whole Google Photos library today?

Ticket: `issues/22-google-photos-access.md`. Sources are Google's own pages, read on **2026-11-12**. No Google account was accessed.

## Bottom line

- **No.** Since **31 March 2025**, no Google API lets an app list or search a user's whole Google Photos library. The Library API can only read media the app uploaded itself. The only way to reach other photos is the **Picker API**, and there **the user** browses, searches and picks in Google's UI. The app can't search, filter or see location.
- Google has posted no Photos API changes since April 2025. The release notes have no 2026 entries.
- So the original idea of Lucy writing a Plugin that queries Google Photos for "Mount Rainier" can't be built on supported APIs. The ways that work are: (1) search **local files** (a local library or a scheduled Google Takeout export) by EXIF GPS, date and filename, or (2) **hand off** to Google Photos' own search in the user's browser, or to the Picker.

## What the Library API allows now

- Scopes removed on 31 March 2025: `photoslibrary.readonly`, `photoslibrary.sharing`, `photoslibrary`. Calls that relied on them return `403 PERMISSION_DENIED`. ([Updates to the Google Photos APIs](https://developers.google.com/photos/support/updates))
- Scopes that remain: `photoslibrary.appendonly`, `photoslibrary.readonly.appcreateddata`, `photoslibrary.edit.appcreateddata`. ([same](https://developers.google.com/photos/support/updates); [Authorization scopes](https://developers.google.com/photos/overview/authorization))
- > "You can now only list, search, and retrieve albums and media items that were created by your app." This covers `albums.list/get` and `mediaItems.list/search`. ([updates](https://developers.google.com/photos/support/updates))
- For search, Google points apps to the Picker API. Announced 18 Sep 2024 to "ensure both user privacy and data protection". ([Google Developers Blog](https://developers.googleblog.com/en/google-photos-picker-api-launch-and-library-api-updates/))
- [Release notes](https://developers.google.com/photos/support/release-notes): the newest entry is 1 Apr 2025, which made the scope removals live and replaced the Acceptable Use Policy with the "Photos API User Data and Developer Policy". There is nothing later.

## What the Picker API allows

- **Flow:** you need an OAuth token with scope `photospicker.mediaitems.readonly`. Lucy creates a session and gets back a `pickerUri`. The user opens it as a link or QR code, then browses or searches their library in Google Photos and picks items. Lucy polls `sessions.get` until `mediaItemsSet` is true, lists the picked items, and downloads them through `baseUrl`. ([Get started](https://developers.google.com/photos/picker/guides/get-started-picker), [scopes](https://developers.google.com/photos/overview/authorization))
- **Limits:** a session can hold up to 2000 items (`maxItemCount`, which defaults to and is capped at 2000). Each session has an `expireTime`, after which its media can't be accessed. ([sessions reference](https://developers.google.com/photos/picker/reference/rest/v1/sessions)) Each `baseUrl` stays active for **60 minutes**. ([media items guide](https://developers.google.com/photos/picker/guides/media-items))
- **Metadata Lucy gets back:** `id`, `createTime`, `type`, `baseUrl`, `mimeType`, `filename`, width and height, camera make and model, and exposure fields. There is **no location, GPS or description field**. ([mediaItems reference](https://developers.google.com/photos/picker/reference/rest/v1/mediaItems))
- **What this means:** Google's search for "mountain" or "Rainier" only runs in Google's UI, driven by the user. Lucy can't run a query, filter by place or date, or confirm that a picked photo shows Rainier, other than by looking at the image itself.

## OAuth and verification for a self-hosted, open-source app

- Google Photos APIs don't support service accounts, so Lucy needs a normal user OAuth flow. On Linux that means a "Desktop app" client. For installed apps, Google says the client secret "is obviously not treated as a secret". ([OAuth 2.0 overview](https://developers.google.com/identity/protocols/oauth2))
- The Photos authorization page says apps using the Photos APIs "must pass the OAuth verification review". ([scopes](https://developers.google.com/photos/overview/authorization)) Google's pages I read don't say whether the Picker scope is classed as "sensitive" or "restricted". **Unconfirmed.**
- **Exemptions from verification:** apps "for your personal use (fewer than 100 users)", and apps in development, testing or staging. Users of these apps click through an "unverified app" warning, and there is a 100-user cap. ([When verification is not needed](https://support.google.com/cloud/answer/13464323))
- **Catch:** if the project's publishing status is "Testing" and its user type is external, it "is issued a refresh token expiring in 7 days". ([OAuth 2.0 overview](https://developers.google.com/identity/protocols/oauth2)) So a bring-your-own-Cloud-project setup should be switched to "In production" without verification. That is allowed for personal use, with the warning screen. Otherwise the user has to sign in again every week.
- **Practical upshot:** Lucy can't ship one shared client ID that all users rely on without going through Google verification. Its user cap is 100 users per client, total. The realistic model is that each user creates their own Google Cloud project and Desktop OAuth client, which is a TUI wizard step. That conflicts with the "configure from whatever they already have" goal.

## Fallbacks compared

| Option | Searches whole library? | Content / place / date search | Setup burden | Terms risk |
|---|---|---|---|---|
| Library API | No (app-created only) | n/a | n/a | n/a |
| Picker API | User does it in Google's UI | User uses Google's search; Lucy gets no location | Own Cloud project + OAuth client per user; click-through warning | Low (supported API) |
| Open `photos.google.com/search/<query>` in the user's browser | Yes, Google's own search, shown to the user | Full Google search | None | Low: the user views a page, nothing is automated |
| Browser automation of photos.google.com | Technically | Full | Fragile scraping of an undocumented UI | **High**: see below |
| Scheduled Google Takeout → local folder, then search | Yes (as of the last export) | GPS/date from EXIF and sidecars; content only with a local vision model | One-time setup in Takeout; archives arrive every 2 months for a year, then must be re-scheduled | Low |
| Local photo library (`~/Pictures`, Immich, digiKam, etc.) | Yes | EXIF GPS, date, filename; content with a local model or the library's own search | Depends on where photos live | None |

- **Browser automation:** Google's Terms (effective 30 Jul 2026) forbid "using automated means to access content from any of our services in violation of the machine-readable instructions" on its pages. ([Google Terms](https://policies.google.com/terms)) `photos.google.com/robots.txt` disallows `/search/`, `/photo/`, `/albums`, `/places` and more (fetched 2026-11-12). So a Plugin that drives the user's logged-in session to search and scrape results goes against those instructions. Don't do it.
- **The search URL:** `https://photos.google.com/search/<query>` is a real route; anonymously it redirects (302) to sign-in. Opening it in the user's default browser is a launch, not automated access. Whether Google's search handles a free-text place like "Mount Rainier" well wasn't tested, because the account wasn't accessed.
- **Takeout:** you can schedule "an archive of your selected data every 2 months for one year". Incremental new and updated data works only when Photos is the only product selected. Archives come as zip or tgz, and the size per archive is configurable (50 GB is an option). Extra metadata goes into "a secondary JSON file". ([Takeout help](https://support.google.com/accounts/answer/3024190)) That page doesn't list the sidecar's field names (e.g. `geoData`). They are widely reported but **not confirmed from a Google source**. Delivery to Drive, Dropbox or OneDrive is a Takeout option, but getting the archive onto disk and unpacking it is still on the user or a sync tool.

## Recommendation for the Mount Rainier scenario

1. Make the success criterion **"find my Mount Rainier photo" in the user's local photo folder(s)**, configured during setup, which may be a Takeout extract. Lucy writes a Plugin that reads EXIF GPS and date, matches against a bounding box around Rainier (and the filename and any Takeout sidecar text), then opens or shows the best matches. This is plain code that Lucy can generate and test, which fits "Plugins are mostly plain code". It needs no Google credentials, and it shows Lucy writing a non-trivial Plugin.
2. For Google Photos users, the Plugin can also **open `photos.google.com/search/Mount Rainier` in the browser**. This is cheap, allowed, and uses Google's content search. Lucy says she has opened the search rather than claiming she found the photo.
3. Leave **Picker API** integration for after v0.1. It needs a per-user Cloud project and OAuth client, and the result is the user picking the photo themselves.
4. **Never** script photos.google.com.

## Open points

- Where do the user's photos actually live: Google Photos only, a phone sync folder, Immich, or something else? This decides whether step 1 works on day one or needs a Takeout first.
- Is the Picker scope sensitive or restricted? This only matters if Lucy ever wants a shared client ID.
- Content search ("photos of mountains") without GPS would need a local vision or CLIP model, a new local Provider. It is out of v0.1 unless wanted.
