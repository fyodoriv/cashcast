-- Opens an xlsx in Numbers, saves it as .numbers, and returns the Forecast
-- table as tab-separated text. Numbers become "c:<cents>", dates YYYY-MM-DD.
on run argv
	set xlsxPath to item 1 of argv
	set numbersPath to item 2 of argv
	set rowsData to {}
	tell application "Numbers"
		launch
		-- Close copies left open by an earlier run, unless they hold unsaved edits.
		set olds to (every document whose name starts with "Cashcast")
		repeat with d in olds
			if modified of d then error "Cashcast is open in Numbers with unsaved changes. Save or close it, then run again."
		end repeat
		repeat with d in olds
			close d saving no
		end repeat
		do shell script "rm -rf " & quoted form of numbersPath
		set opened to open (POSIX file xlsxPath)
		-- On a cold start Numbers can hand back a blank document while the
		-- import opens in another window. Find the imported one by name.
		set doc to missing value
		repeat 120 times
			repeat with d in documents
				set sheetNames to {}
				try
					set sheetNames to name of every sheet of d
				end try
				if (name of d) starts with "Cashcast" and sheetNames contains "Forecast" then
					set doc to contents of d
					exit repeat
				end if
			end repeat
			if doc is not missing value then exit repeat
			delay 0.5
		end repeat
		if doc is missing value then
			set names to {}
			repeat with d in documents
				if (name of d) starts with "Cashcast" or (name of d) starts with "Untitled" then set end of names to (name of d)
			end repeat
			error "The imported workbook did not open within 60 seconds. Cashcast or untitled documents open: " & my joined(names)
		end if
		my closeBlank(opened, doc)
		try
			save doc in (POSIX file numbersPath)
			tell table 1 of sheet "Forecast" of doc
				repeat with r from 1 to (row count)
					-- Copy values out; loop variables here would stay app references.
					set rowValues to value of every cell of row r
					copy rowValues to end of rowsData
				end repeat
			end tell
		on error msg number num
			close doc saving no
			error msg number num
		end try
		close doc saving no
	end tell
	set out to ""
	repeat with r from 1 to count of rowsData
		set rowValues to item r of rowsData
		set rowText to ""
		repeat with i from 1 to count of rowValues
			set rowText to rowText & show(item i of rowValues) & tab
		end repeat
		set out to out & rowText & linefeed
	end repeat
	return out
end run

-- Closes the document `open` returned when it is an unchanged blank one
-- that is not the import. Never closes anything else.
on closeBlank(opened, doc)
	tell application "Numbers"
		try
			if (id of opened) is (id of doc) then return
			if (name of opened) does not start with "Untitled" then return
			if (name of every sheet of opened) is not {"Sheet 1"} then return
			if modified of opened then return
			close opened saving no
		end try
	end tell
end closeBlank

on joined(theList)
	set AppleScript's text item delimiters to ", "
	set t to theList as text
	set AppleScript's text item delimiters to ""
	return t
end joined

on show(x)
	if x is missing value then return ""
	if class of x is real or class of x is integer then
		-- Cents, rounded half away from zero, without Standard Additions.
		set c to x * 100
		if c < 0 then
			set c to (c - 0.5) div 1
		else
			set c to (c + 0.5) div 1
		end if
		return "c:" & (c as text)
	end if
	if class of x is date then
		return ((year of x) as text) & "-" & text -2 thru -1 of ("0" & ((month of x) as integer)) & "-" & text -2 thru -1 of ("0" & (day of x))
	end if
	return x as text
end show
