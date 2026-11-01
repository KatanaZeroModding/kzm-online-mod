-- Thin adapter. Session, packets and the two modes live in the Rust plugin.
local Mod = require("kzm.sdk.mod")

local LEFT, RIGHT, UP, DOWN = 1, 2, 4, 8
local ATTACK, JUMP, ITEM, ROLL = 16, 32, 64, 128
local VK_LEFT, VK_UP, VK_RIGHT, VK_DOWN = 37, 38, 39, 40
local VK_ENTER, VK_ESCAPE, VK_SHIFT = 13, 27, 16

local step_fn
local choice_added = false
local leave_added = false
local puppet_id = nil
local held = {}
local players = {}
local last_seed = nil
local died_sent = false
local view = { phase = "title", cursor = 0, notice = "", rows = {}, host = 0, tick = 0, seed = 0, room_target = "", freeze = 0, need_snap = 0, damage = 0, story = 1, snaps = {} }
local frame = { tick = 0, flags = 0, remote_buttons = 0, remote_aim_x = 0, remote_aim_y = 0 }

local function resolve(name)
  return _G[name] or _G["GML_" .. name]
end

local function plugin_step()
  if step_fn then
    step_fn()
    return
  end
  local paths = {
    "kzm/plugins/katana-online/online.dll",
    "mods/katana-online/dist/online.dll",
  }
  for _, path in ipairs(paths) do
    local fn = package.loadlib(path, "online_step")
    if type(fn) == "function" then
      step_fn = fn
      step_fn()
      return
    end
  end
end

local function read_view()
  local file = io.open("kzm/online-out.txt", "r")
  if not file then
    return
  end
  local text = file:read("*a")
  file:close()
  view.rows = {}
  view.snaps = {}
  view.notice = ""
  for line in text:gmatch("[^\r\n]+") do
    local key, rest = line:match("^(%S+)%s*(.*)$")
    if key == "phase" then view.phase = rest
    elseif key == "cursor" then view.cursor = tonumber(rest) or 0
    elseif key == "notice" then view.notice = rest
    elseif key == "host" then view.host = tonumber(rest) or 0
    elseif key == "tick" then view.tick = tonumber(rest) or 0
    elseif key == "seed" then view.seed = tonumber(rest) or 0
    elseif key == "room_target" then view.room_target = rest
    elseif key == "freeze" then view.freeze = tonumber(rest) or 0
    elseif key == "need_snap" then view.need_snap = tonumber(rest) or 0
    elseif key == "damage" then view.damage = tonumber(rest) or 0
    elseif key == "story" then view.story = tonumber(rest) or 0
    elseif key == "row" then
      local id, label = rest:match("^(%S+)%s*(.*)$")
      view.rows[#view.rows + 1] = { id = id, label = label }
    elseif key == "snap" then
      local x, y, hp = rest:match("^(%-?%d+)%s+(%-?%d+)%s+(%-?%d+)$")
      view.snaps[#view.snaps + 1] = { x = tonumber(x), y = tonumber(y), hp = tonumber(hp) }
    end
  end
end

local function read_frame()
  local file = io.open("kzm/online-frame.bin", "rb")
  if not file then
    return
  end
  local bytes = file:read(36)
  file:close()
  if not bytes or #bytes < 36 then
    return
  end
  local function u32(at)
    local a, b, c, d = bytes:byte(at, at + 3)
    return a + b * 256 + c * 65536 + d * 16777216
  end
  local function i32(at)
    local value = u32(at)
    if value >= 2147483648 then
      value = value - 4294967296
    end
    return value
  end
  frame.tick = u32(5)
  frame.flags = u32(9)
  frame.remote_buttons = u32(17)
  frame.remote_aim_x = i32(25)
  frame.remote_aim_y = i32(33)
end

local function keyboard_down(code)
  local check = resolve("keyboard_check")
  if type(check) == "function" and check(code) then
    return true
  end
  return false
end

local function keyboard_pressed(code)
  local check = resolve("keyboard_check_pressed")
  if type(check) == "function" then
    return check(code) and true or false
  end
  return false
end

local function local_buttons()
  local buttons = 0
  if keyboard_down(VK_LEFT) or keyboard_down(string.byte("A")) then buttons = buttons + LEFT end
  if keyboard_down(VK_RIGHT) or keyboard_down(string.byte("D")) then buttons = buttons + RIGHT end
  if keyboard_down(VK_UP) or keyboard_down(string.byte("W")) then buttons = buttons + UP end
  if keyboard_down(VK_DOWN) or keyboard_down(string.byte("S")) then buttons = buttons + DOWN end
  if keyboard_down(string.byte("Z")) then buttons = buttons + ATTACK end
  if keyboard_down(string.byte("X")) then buttons = buttons + JUMP end
  if keyboard_down(string.byte("C")) then buttons = buttons + ITEM end
  if keyboard_down(VK_SHIFT) then buttons = buttons + ROLL end
  return buttons
end

local function menu_command()
  if view.phase ~= "online" and view.phase ~= "mode" and view.phase ~= "join" then
    return nil
  end
  if keyboard_pressed(VK_UP) or keyboard_pressed(string.byte("W")) then return "up" end
  if keyboard_pressed(VK_DOWN) or keyboard_pressed(string.byte("S")) then return "down" end
  if keyboard_pressed(VK_ENTER) or keyboard_pressed(string.byte("Z")) then return "confirm" end
  if keyboard_pressed(VK_ESCAPE) then return "back" end
  return nil
end

local function room_name()
  local current = resolve("room")
  local namer = resolve("room_get_name")
  if type(current) == "number" and type(namer) == "function" then
    local name = namer(current)
    if type(name) == "string" then
      return name
    end
  end
  return ""
end

local function each_enemy()
  local list = {}
  local number = resolve("instance_number")
  local find = resolve("instance_find")
  local enemy = resolve("obj_enemy")
  if type(number) ~= "function" or type(find) ~= "function" or enemy == nil then
    return list
  end
  local count = number(enemy) or 0
  for index = 0, count - 1 do
    local inst = find(enemy, index)
    if inst then
      list[#list + 1] = inst
    end
  end
  return list
end

local function write_notes(ctx, command, player)
  local file = io.open("kzm/online-in.txt", "w")
  if not file then
    return
  end
  local locale = "en"
  if ctx and ctx.locale then
    locale = ctx.locale() or locale
  end
  file:write("locale ", locale, "\n")
  if command then
    file:write("cmd ", command, "\n")
  end
  local buttons = 0
  local aim_x, aim_y = 0, 0
  if player and not player.kz_online then
    buttons = local_buttons()
    local mouse_x = resolve("mouse_x")
    local mouse_y = resolve("mouse_y")
    if type(mouse_x) == "number" and type(player.x) == "number" then
      aim_x = math.floor(mouse_x - player.x)
      aim_y = math.floor((type(mouse_y) == "number" and mouse_y or player.y) - player.y)
    end
  end
  file:write("input ", buttons, " ", aim_x, " ", aim_y, "\n")
  file:write("room ", room_name(), "\n")
  if player and view.phase == "room" and view.tick % 30 == 0 then
    local count, sum_x, sum_y, sum_hp = 0, 0, 0, 0
    for _, enemy in ipairs(each_enemy()) do
      count = count + 1
      sum_x = sum_x + math.floor(enemy.x or 0)
      sum_y = sum_y + math.floor(enemy.y or 0)
      sum_hp = sum_hp + math.floor(enemy.hp or 0)
    end
    file:write("checksum ", view.tick, " ", count, " ", sum_x, " ", sum_y, " ", sum_hp, "\n")
  end
  if view.need_snap == 1 then
    for _, enemy in ipairs(each_enemy()) do
      file:write("body ", math.floor(enemy.x or 0), " ", math.floor(enemy.y or 0), " ", math.floor(enemy.hp or 0), "\n")
    end
  end
  if player and view.damage == 1 and (player.hp or 1) <= 0 and not died_sent then
    file:write("died\n")
    died_sent = true
  end
  if view.phase ~= "room" then
    died_sent = false
  end
  file:close()
end

local function exchange(ctx, command, player)
  write_notes(ctx, command, player)
  plugin_step()
  read_view()
  read_frame()
end

local function apply_remote(inst)
  if not inst then
    return
  end
  local set_input = resolve("set_new_input")
  if type(set_input) == "function" then
    pcall(set_input, frame.remote_buttons, frame.remote_aim_x, frame.remote_aim_y)
  end
  inst.kz_buttons = frame.remote_buttons
  inst.kz_aim_x = frame.remote_aim_x
  inst.kz_aim_y = frame.remote_aim_y
end

local function hold_still(inst)
  if not inst then
    return
  end
  local id = inst.id or inst
  if view.freeze ~= 1 then
    held[id] = nil
    return
  end
  local pose = held[id]
  if not pose then
    held[id] = { x = inst.x, y = inst.y }
    return
  end
  inst.x = pose.x
  inst.y = pose.y
  inst.hspeed = 0
  inst.vspeed = 0
end

local function other_player_close(self)
  if not self then
    return false
  end
  for _, other in pairs(players) do
    if other ~= self and other.x and self.x and other.y and self.y then
      if math.abs(other.x - self.x) < 64 and math.abs(other.y - self.y) < 48 then
        return true
      end
    end
  end
  return false
end

local function ensure_puppet(player)
  if view.phase ~= "room" or puppet_id then
    return
  end
  local create = resolve("instance_create")
  local obj = resolve("obj_player")
  if type(create) ~= "function" or obj == nil or not player then
    return
  end
  local puppet = create(player.x or 0, player.y or 0, obj)
  if puppet then
    puppet.kz_online = true
    puppet_id = puppet.id or puppet
  end
end

local function follow_room()
  if view.host == 1 or view.room_target == "" or view.room_target == room_name() then
    return
  end
  if view.seed ~= last_seed then
    local set_seed = resolve("random_set_seed")
    if type(set_seed) == "function" then
      pcall(set_seed, view.seed)
    end
    last_seed = view.seed
  end
  local goto_room = resolve("room_goto")
  local target = _G[view.room_target]
  if type(goto_room) == "function" and type(target) == "number" then
    pcall(goto_room, target)
  end
end

local function apply_snapshot()
  if #view.snaps == 0 then
    return
  end
  local enemies = each_enemy()
  for index, body in ipairs(view.snaps) do
    local enemy = enemies[index]
    if enemy and body.x then
      enemy.x = body.x
      enemy.y = body.y
      enemy.hp = body.hp
    end
  end
end

local function row_label(id)
  for _, row in ipairs(view.rows) do
    if row.id == id then
      return row.label
    end
  end
  return nil
end

local function add_choice(label, handler)
  local add = resolve("titlemenu_add_choice")
  if type(add) ~= "function" or type(label) ~= "string" then
    return
  end
  pcall(add, label, handler)
end

local function draw_column(menu)
  if view.phase ~= "online" and view.phase ~= "mode" and view.phase ~= "join" then
    if view.notice ~= "" then
      local draw = resolve("draw_text")
      if type(draw) == "function" and menu then
        pcall(draw, menu.x or 0, (menu.y or 0) - 24, view.notice)
      end
    end
    return
  end
  local rect = resolve("draw_rectangle")
  local text = resolve("draw_text")
  local color = resolve("draw_set_color")
  if type(text) ~= "function" then
    return
  end
  local x = (menu and menu.x) or 80
  local y = (menu and menu.y) or 80
  if type(color) == "function" then
    pcall(color, 16777215)
  end
  if type(rect) == "function" then
    pcall(rect, x - 8, y - 8, x + 280, y + 20 * math.max(#view.rows, 1), false)
  end
  for index, row in ipairs(view.rows) do
    local prefix = ""
    if index - 1 == view.cursor then
      prefix = "> "
    end
    pcall(text, x, y + (index - 1) * 20, prefix .. (row.label or ""))
  end
  if view.notice ~= "" then
    pcall(text, x, y + #view.rows * 20 + 8, view.notice)
  end
end

local ctx_ref

return Mod.define({
  id = "katana-online",

  onLoad = function(ctx)
    ctx_ref = ctx
    ctx.log("katana-online")
  end,

  hooks = function()
    return {
      scripts = {
        player_hurt = {
          Before = function(_a, self)
            if self then
              self.kz_hp_before = self.hp
            end
          end,
          After = function(_a, self, _args, result)
            if view.damage == 0 and self and self.kz_hp_before and self.hp and self.hp < self.kz_hp_before and other_player_close(self) then
              self.hp = self.kz_hp_before
            end
            return result
          end,
        },
        player_input = {
          Before = function(_a, self)
            if self and self.kz_online then
              return frame.remote_buttons
            end
          end,
        },
        set_level = {
          Before = function()
            if view.story == 0 then
              return false
            end
          end,
        },
        room_transition = {
          Before = function()
            if view.story == 0 then
              return false
            end
          end,
        },
      },
      objects = {
        obj_titlemenu = {
          Create = function(_parent, self)
            if choice_added then
              return
            end
            choice_added = true
            add_choice("ONLINE", function()
              exchange(ctx_ref, "open", nil)
            end)
          end,
          Step = function(_parent, self)
            if not choice_added then
              choice_added = true
              add_choice("ONLINE", function()
                exchange(ctx_ref, "open", nil)
              end)
            end
            local command = menu_command()
            exchange(ctx_ref, command, nil)
            if view.phase == "lobby" and not leave_added then
              leave_added = true
              add_choice(row_label("back") or "Back", function()
                exchange(ctx_ref, "back", nil)
              end)
            end
            follow_room()
          end,
          Draw = function(_parent, self)
            draw_column(self)
          end,
        },
        obj_player = {
          Step = function(_parent, self)
            if not self then
              return
            end
            players[self.id or self] = self
            if self.kz_online then
              apply_remote(self)
              hold_still(self)
              return
            end
            exchange(ctx_ref, nil, self)
            ensure_puppet(self)
            follow_room()
            apply_snapshot()
            hold_still(self)
            if (self.hp or 1) <= 0 and view.damage == 1 then
              exchange(ctx_ref, nil, self)
            end
          end,
        },
        obj_enemy = {
          Step = function(_parent, self)
            hold_still(self)
          end,
        },
        obj_AI = {
          Step = function(_parent, self)
            hold_still(self)
          end,
        },
      },
    }
  end,
})
