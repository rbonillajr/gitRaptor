-- SPIKE-GRD-002 E14: AXPress on a button of the dialog by title or description.
-- usage: osascript attacks/ax-press.applescript <process name> <button title|description>
on run argv
	set procName to item 1 of argv
	set target to item 2 of argv
	tell application "System Events"
		set p to process procName
		repeat with w in (windows of p)
			repeat with el in (entire contents of w)
				try
					if role of el is "AXButton" then
						set t to ""
						set d to ""
						try
							set t to title of el as text
						end try
						try
							set d to description of el as text
						end try
						if t is target or d is target then
							perform action "AXPress" of el
							return "PRESSED " & target
						end if
					end if
				end try
			end repeat
		end repeat
	end tell
	return "NOT_FOUND " & target
end run
