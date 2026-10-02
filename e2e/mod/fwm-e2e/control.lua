-- Drives the factorio-window-memory end-to-end test. Opens built-in windows one
-- after another (each one runs agui::Window::center, which the plugin hooks),
-- screenshots them, then holds the character window open so the harness can
-- drag it. Talks to the harness through files in script-output/fwm-e2e/.

local OUT = "fwm-e2e/"
local SETTLE_TICKS = 120      -- let the game finish loading before starting
local STEP_TICKS = 90         -- open, screenshot at +45, close at +75
local HOLD_TICKS = 60 * 20    -- how long the harness gets to drag the character window

local function write(name, text)
  helpers.write_file(OUT .. name, text, false)
end

local function screenshot(player, name)
  game.take_screenshot{
    player = player,
    by_player = player,
    path = OUT .. name .. ".png",
    show_gui = true,
    resolution = { player.display_resolution.width, player.display_resolution.height },
    force_render = true,
  }
end

local function place(player, name)
  local surface = player.surface
  local position = surface.find_non_colliding_position(name, player.position, 8, 0.5)
  if not position then return nil end
  return surface.create_entity{ name = name, position = position, force = player.force }
end

local STEPS = {
  { name = "character",  open = function(p) p.opened = defines.gui_type.controller end },
  { name = "chest",      open = function(p) p.opened = storage.chest end },
  { name = "assembler",  open = function(p) p.opened = storage.assembler end },
  { name = "furnace",    open = function(p) p.opened = storage.furnace end },
  { name = "production", open = function(p) p.opened = defines.gui_type.production end },
}

script.on_init(function()
  -- Skip freeplay's intro cutscene and crash site so the character is usable at once.
  if remote.interfaces.freeplay then
    pcall(remote.call, "freeplay", "set_disable_crashsite", true)
    pcall(remote.call, "freeplay", "set_skip_intro", true)
  end
end)

local function ready(player)
  if not player or not player.connected then return false end
  if player.controller_type == defines.controllers.cutscene then player.exit_cutscene() end
  if not player.character then player.create_character() end
  return player.character ~= nil
end

local function begin(player)
  storage.chest = place(player, "wooden-chest")
  if storage.chest then storage.chest.insert{ name = "iron-plate", count = 50 } end
  storage.assembler = place(player, "assembling-machine-1")
  storage.furnace = place(player, "stone-furnace")
  storage.start = game.tick + 1  -- step 1 opens on the next tick
  storage.opened = {}
  write("started.txt", tostring(game.tick))
end

script.on_event(defines.events.on_tick, function(event)
  local player = game.get_player(1)
  if storage.finished then return end
  if not storage.start then
    if event.tick >= SETTLE_TICKS and ready(player) then begin(player) end
    return
  end

  local t = event.tick - storage.start
  local index = math.floor(t / STEP_TICKS) + 1
  local phase = t % STEP_TICKS
  local step = STEPS[index]
  if step then
    if phase == 0 then
      step.open(player)
    elseif phase == 5 then
      -- player.opened reads nil for non-entity windows like the character screen
      storage.opened[step.name] = player.opened_gui_type ~= defines.gui_type.none
    elseif phase == 45 then
      screenshot(player, string.format("%02d-%s", index, step.name))
    elseif phase == 75 then
      player.opened = nil
    end
    return
  end

  -- Hold the character window open for the harness to drag, then reopen it to
  -- check it comes back where it was dragged.
  local h = t - #STEPS * STEP_TICKS
  if h == 0 then
    player.opened = defines.gui_type.controller
    write("hold.txt", tostring(game.tick))
  elseif h == HOLD_TICKS then
    screenshot(player, "90-character-after-drag")
    player.opened = nil
  elseif h == HOLD_TICKS + 30 then
    player.opened = defines.gui_type.controller
  elseif h == HOLD_TICKS + 75 then
    screenshot(player, "91-character-reopened")
  elseif h == HOLD_TICKS + 105 then
    player.opened = nil
    storage.finished = true
    write("done.json", helpers.table_to_json{ opened = storage.opened, tick = game.tick })
  end
end)
