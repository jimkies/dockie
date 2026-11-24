-- Written by Dockie. Dockie rewrites this file when its settings change, so edit it there.
-- 1. Anti-AFK: turns off Roblox's idle kick for this client.
-- 2. If the executor has WebSocket, tells Dockie this account is attached (localhost only).

local Players = game:GetService("Players")
local HttpService = game:GetService("HttpService")

if not game:IsLoaded() then game.Loaded:Wait() end
local lp = Players.LocalPlayer
while not lp do
	task.wait(0.5)
	lp = Players.LocalPlayer
end

pcall(function()
	for _, c in ipairs(getconnections(lp.Idled)) do
		pcall(function() c:Disable() end)
	end
end)

local ws_api = (typeof(WebSocket) == "table" or type(WebSocket) == "table") and WebSocket or nil
if not (ws_api and ws_api.connect) then return end

task.spawn(function()
	while true do
		pcall(function()
			local ws = ws_api.connect("ws://127.0.0.1:__PORT__")
			local open = true
			pcall(function() ws.OnClose:Connect(function() open = false end) end)
			ws:Send(HttpService:JSONEncode({
				type = "hello", userId = lp.UserId, username = lp.Name, placeId = game.PlaceId, jobId = game.JobId,
			}))
			while open do
				task.wait(10)
				if not pcall(function() ws:Send(HttpService:JSONEncode({ type = "heartbeat", userId = lp.UserId })) end) then
					open = false
				end
			end
			pcall(function() ws:Close() end)
		end)
		task.wait(15)
	end
end)
