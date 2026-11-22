# Google Sheets playbook

## Before you start

- You need a Google account with access to the sheet.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- Never write amounts from `private/` into a sheet that other people can see unless you ask for it.

## Export your data

1. Open the spreadsheet.
2. Click the tab you want. CSV saves the current tab only.
3. **File > Download > Comma-separated values (.csv)**.
4. Save it into `private/users/<you>/inbox/`.

## Actions

### download-sheet-csv

1. Follow "Export your data" for each tab.
2. Verify: the file name ends with the tab name and `.csv`.

### api-read-values

1. Call `GET /v4/spreadsheets/{spreadsheetId}/values/{range}` with the `spreadsheets.readonly` scope.
2. Keep results only under `private/users/<you>/`.

### write-forecast-tab

1. Show the user the target tab name and the range that will change. Use a new tab, never overwrite the user's own tabs.
2. Ask the user to confirm this exact change before clicking the final button (or before sending the API call).
3. API: `values.update` with the `spreadsheets` scope.

## API

1. In Google Cloud Console, create a project you own. Enable the Google Sheets API.
2. Create an OAuth client of type **Desktop app**. Download the client file into `private/users/<you>/`.
3. Ask for `spreadsheets.readonly`. Ask for `spreadsheets` only if you want the forecast tab written.
4. Put `CASHCAST_GOOGLE_CLIENT_SECRET_FILE=...` and `CASHCAST_GOOGLE_SHEET_ID=...` in `private/users/<you>/.env`. Never commit them.

## Sources

- https://developers.google.com/workspace/sheets/api/reference/rest/v4/spreadsheets.values/get
- https://developers.google.com/workspace/sheets/api/reference/rest/v4/spreadsheets.values/update
- https://support.google.com/docs/answer/49114

Last checked: 2026-10-18
