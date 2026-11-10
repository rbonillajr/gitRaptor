-- SPIKE-GRD-002 E14/E9: list the accessibility tree a same-user process sees for the dialog owner.
-- usage: osascript attacks/ax-dump.applescript <process name>
on run argv
	set procName to item 1 of argv
	set out to ""
	tell application "System Events"
		if not (exists process procName) then return "NO_PROCESS " & procName
		set p to process procName
		set out to out & "frontmost=" & (frontmost of p as text) & linefeed
		repeat with w in (windows of p)
			set out to out & "WINDOW name=" & (name of w as text) & linefeed
			try
				set els to entire contents of w
			on error e
				set out to out & "ERR entire contents: " & e & linefeed
				set els to {}
			end try
			repeat with el in els
				set r to "?"
				set t to ""
				set v to ""
				set d to ""
				set pos to ""
				set sz to ""
				try
					set r to role of el
				end try
				try
					set t to title of el as text
				end try
				try
					set v to value of el as text
				end try
				try
					set d to description of el as text
				end try
				try
					set pos to (item 1 of (position of el as list) as text) & "," & (item 2 of (position of el as list) as text)
					set sz to (item 1 of (size of el as list) as text) & "," & (item 2 of (size of el as list) as text)
				end try
				set out to out & "EL role=" & r & " | title=" & t & " | value=" & v & " | desc=" & d & " | pos=" & pos & " | size=" & sz & linefeed
			end repeat
		end repeat
	end tell
	return out
end run
