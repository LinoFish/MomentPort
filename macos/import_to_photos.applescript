-- MomentPort v1.1 AppleScript 兜底导入脚本
-- 用法：osascript import_to_photos.applescript "/path/to/输出文件夹/Burst_01"
--
-- 只做一件事：把该文件夹里的照片加进 Photos。
-- 不声称成组：成组与否必须在 macOS 上用 PhotoKit 回查 burstIdentifier /
-- representsBurst（见 macos/BRIDGE.md），本脚本不做回查、不改文件、
-- 不写 BurstUUID、不改相册数据库。
-- 注意：本脚本用 Finder 展开文件夹；若 Photos 弹授权请在系统设置里允许。

on run argv
	if (count of argv) < 1 then
		return "用法：osascript import_to_photos.applescript \"/path/to/Burst_01\""
	end if
	set folderPath to item 1 of argv
	tell application "Finder"
		set theFolder to POSIX file folderPath as alias
		set fileList to every file of theFolder
		set importPaths to {}
		repeat with f in fileList
			set end of importPaths to (POSIX path of (f as alias))
		end repeat
	end tell
	if (count of importPaths) = 0 then
		return "文件夹里没有文件，未导入任何照片。"
	end if
	tell application "Photos"
		activate
		-- skip check duplicates 设为 false：不跳过重复项，重复由用户在 Photos 里处理。
		import importPaths skip check duplicates false
	end tell
	return "已把 " & (count of importPaths) & " 个文件交给 Photos 导入（仅导入，不声称已成组，请用 PhotoKit 回查确认）。"
end run
