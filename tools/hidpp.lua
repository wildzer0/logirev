--[[
  Wireshark dissector for Logitech HID++ (as captured by logirev).

  logirev stores HID++ reports as USB interrupt transfers (USBPcap link
  type). Without this plugin Wireshark shows them as raw bytes; with it, each
  packet is decoded: report type, device slot, feature, function, errors and
  the most common notifications.

  Installation: copy this file into Wireshark's personal Lua plugins folder
  (Help > About Wireshark > Folders > "Personal Lua Plugins"), then restart
  Wireshark or press Ctrl+Shift+L. Display filter: "hidpp".

  Feature names are learnt from the feature discovery that logirev performs
  when a device connects, so start the capture before the device connects
  (logirev does this automatically when the capture is enabled in its
  configuration).
]]

local hidpp = Proto("hidpp", "Logitech HID++")

local REPORTS = { [0x10] = "Short", [0x11] = "Long", [0x12] = "Very long" }
local SUB_IDS = {
  [0x40] = "Device disconnection", [0x41] = "Device connection",
  [0x80] = "Set register", [0x81] = "Get register",
  [0x82] = "Set long register", [0x83] = "Get long register",
  [0x8F] = "HID++ 1.0 error",
}
local ERR10 = { [1] = "InvalidSubId", [2] = "InvalidAddress", [3] = "InvalidValue",
  [4] = "ConnectFail", [5] = "TooManyDevices", [6] = "AlreadyExists", [7] = "Busy",
  [8] = "UnknownDevice", [9] = "ResourceError", [10] = "RequestUnavailable",
  [11] = "InvalidParamValue", [12] = "WrongPinCode" }
local ERR20 = { [1] = "Unknown", [2] = "InvalidArgument", [3] = "OutOfRange",
  [4] = "HardwareError", [5] = "NotAllowed", [6] = "InvalidFeatureIndex",
  [7] = "InvalidFunctionId", [8] = "Busy", [9] = "Unsupported" }
local FEATURES = {
  [0x0000] = "Root", [0x0001] = "FeatureSet", [0x0003] = "DeviceFwInfo",
  [0x0005] = "DeviceNameType", [0x1000] = "BatteryStatus", [0x1004] = "UnifiedBattery",
  [0x1814] = "ChangeHost", [0x1815] = "HostsInfo", [0x1B04] = "ReprogControlsV4",
  [0x1D4B] = "WirelessDeviceStatus", [0x2110] = "SmartShift", [0x2121] = "HiResWheel",
  [0x2201] = "AdjustableDpi", [0x2205] = "PointerSpeed", [0x8060] = "ReportRate",
  [0x8061] = "ExtendedReportRate", [0x8100] = "OnboardProfiles",
}

local f = {
  report   = ProtoField.uint8("hidpp.report", "Report", base.HEX, REPORTS),
  device   = ProtoField.uint8("hidpp.device", "Device index", base.HEX),
  sub_id   = ProtoField.uint8("hidpp.sub_id", "Sub-ID (HID++ 1.0)", base.HEX, SUB_IDS),
  address  = ProtoField.uint8("hidpp.address", "Register", base.HEX),
  findex   = ProtoField.uint8("hidpp.feature_index", "Feature index", base.DEC),
  feature  = ProtoField.uint16("hidpp.feature", "Feature", base.HEX, FEATURES),
  func     = ProtoField.uint8("hidpp.function", "Function", base.DEC, nil, 0xF0),
  swid     = ProtoField.uint8("hidpp.swid", "Software ID", base.HEX, nil, 0x0F),
  error10  = ProtoField.uint8("hidpp.error10", "Error", base.HEX, ERR10),
  error20  = ProtoField.uint8("hidpp.error20", "Error", base.HEX, ERR20),
  cid      = ProtoField.uint16("hidpp.cid", "Button (control id)", base.HEX),
  params   = ProtoField.bytes("hidpp.params", "Parameters"),
}
hidpp.fields = f

-- Direction of the USBPcap transfer: 0 = host->device, 1 = device->host.
local ok_dir, f_dir = pcall(Field.new, "usb.irp_info.direction")

-- Learnt state: features[device][index] = feature id.
local features = {}
-- Pending feature lookups: lookups[device .. ":" .. b3] = feature id asked.
local lookups = {}
local featureset_index = {}

local function feature_of(dev, index)
  if index == 0 then return 0x0000 end
  return features[dev] and features[dev][index]
end

local function learn(dev, index, id)
  features[dev] = features[dev] or {}
  features[dev][index] = id
  if id == 0x0001 then featureset_index[dev] = index end
end

function hidpp.dissector(tvb, pinfo, tree)
  local len = tvb:len()
  if len < 4 then return 0 end
  local rid = tvb(0, 1):uint()
  if not REPORTS[rid] then return 0 end

  pinfo.cols.protocol = "HID++"
  local dev, b2, b3 = tvb(1, 1):uint(), tvb(2, 1):uint(), tvb(3, 1):uint()
  local dir = nil
  if ok_dir and f_dir() then dir = f_dir()() end
  local t = tree:add(hidpp, tvb(), "Logitech HID++")
  t:add(f.report, tvb(0, 1))
  t:add(f.device, tvb(1, 1))
  local info

  if b2 == 0x8F and len >= 6 then
    t:add(f.sub_id, tvb(2, 1))
    t:add(f.error10, tvb(5, 1))
    info = string.format("ERROR 1.0 %s (sub-id 0x%02X)", ERR10[tvb(5, 1):uint()] or "?", tvb(3, 1):uint())
  elseif b2 == 0xFF and len >= 6 then
    t:add(f.error20, tvb(5, 1))
    info = string.format("ERROR 2.0 %s (feature index %d)", ERR20[tvb(5, 1):uint()] or "?", tvb(3, 1):uint())
  elseif SUB_IDS[b2] then
    t:add(f.sub_id, tvb(2, 1))
    t:add(f.address, tvb(3, 1))
    info = string.format("%s 0x%02X", SUB_IDS[b2], b3)
    if b2 == 0x41 and len >= 5 then
      local up = math.floor(tvb(4, 1):uint() / 0x40) % 2 == 0
      info = info .. (up and " (link up)" or " (link down)")
    end
  else
    t:add(f.findex, tvb(2, 1))
    local fid = feature_of(dev, b2)
    if fid then t:add(f.feature, tvb(2, 1), fid):set_generated() end
    t:add(f.func, tvb(3, 1))
    t:add(f.swid, tvb(3, 1))
    local fn, sw = math.floor(b3 / 16), b3 % 16
    local name = fid and (FEATURES[fid] or string.format("0x%04X", fid)) or string.format("index %d", b2)
    info = string.format("%s fn %d", name, fn)
    if sw == 0 then info = info .. " (notification)" end

    -- Learn feature indexes from Root.getFeature and FeatureSet.getFeatureId.
    if not pinfo.visited and len >= 6 then
      local key = dev .. ":" .. b2 .. ":" .. b3
      if dir == 0 then
        if b2 == 0 and fn == 0 then lookups[key] = tvb(4, 2):uint() end
        if featureset_index[dev] == b2 and fn == 1 then lookups[key] = -tvb(4, 1):uint() end
      elseif dir == 1 and lookups[key] then
        local asked = lookups[key]
        if asked >= 0 then
          if tvb(4, 1):uint() ~= 0 then learn(dev, tvb(4, 1):uint(), asked) end
        else
          learn(dev, -asked, tvb(4, 2):uint())
        end
        lookups[key] = nil
      end
    end

    -- Diverted button events: list the held control ids.
    if fid == 0x1B04 and fn == 0 and sw == 0 then
      local held = {}
      for off = 4, math.min(len, 12) - 2, 2 do
        local cid = tvb(off, 2):uint()
        if cid ~= 0 then
          t:add(f.cid, tvb(off, 2))
          held[#held + 1] = string.format("0x%04X", cid)
        end
      end
      info = "Buttons held: " .. (#held > 0 and table.concat(held, " ") or "none")
    end
  end

  if len > 4 then t:add(f.params, tvb(4)) end
  local arrow = dir == 0 and "-> " or (dir == 1 and "<- " or "")
  pinfo.cols.info = string.format("%sdev 0x%02X  %s", arrow, dev, info)
  return len
end

-- Hook into the USB dissector. The heuristic accepts reports whose size
-- matches their HID++ report id (7, 20 or 64 bytes).
local SIZES = { [0x10] = 7, [0x11] = 20, [0x12] = 64 }
local function heuristic(tvb, pinfo, tree)
  if tvb:len() < 4 then return false end
  local want = SIZES[tvb(0, 1):uint()]
  if not want or tvb:len() ~= want then return false end
  hidpp.dissector(tvb, pinfo, tree)
  return true
end
hidpp:register_heuristic("usb.interrupt", heuristic)
pcall(function() DissectorTable.get("usb.interrupt"):add(0xffff, hidpp) end)

function hidpp.init()
  features, lookups, featureset_index = {}, {}, {}
end
