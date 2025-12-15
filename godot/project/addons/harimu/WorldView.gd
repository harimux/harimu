extends Node3D

const SCALE = 0.5
const AGENT_COLOR = Color(0.3, 0.8, 1.0, 0.8)
const ORE_QI_COLOR = Color(0.2, 1.0, 0.6, 0.8)
const ORE_TRANSISTOR_COLOR = Color(1.0, 0.65, 0.25, 0.8)
const STRUCTURE_COLOR = Color(0.9, 0.9, 0.9, 0.8)
const PLAY_INTERVAL = 0.6

var snapshots = []
var current_index = 0
var playing = false
var accum = 0.0
var world_root
var label
var camera

func _ready():
	world_root = Node3D.new()
	add_child(world_root)

	label = Label.new()
	var layer = CanvasLayer.new()
	layer.add_child(label)
	add_child(layer)

	_add_camera()
	_add_light()
	_add_ground()

	snapshots = _load_snapshots()
	if snapshots.size() == 0:
		push_error("No snapshots available; run the simulation first.")
		return

	_show_snapshot(0)

func _process(delta):
	if playing and snapshots.size() > 0:
		accum += delta
		if accum >= PLAY_INTERVAL:
			accum -= PLAY_INTERVAL
			_advance(1)

func _unhandled_input(event):
	if event is InputEventKey and event.pressed:
		if event.ctrl_pressed:
			if event.keycode == KEY_EQUAL or event.keycode == KEY_PLUS:
				_zoom(-1)
				return
			elif event.keycode == KEY_MINUS:
				_zoom(1)
				return
		match event.keycode:
			KEY_RIGHT:
				_advance(1)
			KEY_LEFT:
				_advance(-1)
			KEY_SPACE:
				playing = not playing
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_WHEEL_UP:
			_zoom(-1)
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			_zoom(1)

func _render_snapshot(snapshot):
	for child in world_root.get_children():
		child.queue_free()
	for node in snapshot.get("ore_nodes", []):
		var pos = _v3(node.get("position", Vector3.ZERO))
		var color = ORE_QI_COLOR
		if node.get("ore", "qi") == "transistor":
			color = ORE_TRANSISTOR_COLOR
		_spawn_box(pos, Vector3.ONE * SCALE, color, "ore")

	for structure in snapshot.get("structures", []):
		var pos = _v3(structure.get("position", Vector3.ZERO))
		_spawn_box(pos + Vector3(0, SCALE * 0.5, 0), Vector3.ONE * SCALE, STRUCTURE_COLOR, "structure")

	for agent in snapshot.get("agents", []):
		var pos = _v3(agent.get("position", Vector3.ZERO))
		_spawn_box(pos + Vector3(0, SCALE, 0), Vector3.ONE * (SCALE * 0.75), AGENT_COLOR, "agent")

func _spawn_box(pos, size, color, kind):
	var mesh = BoxMesh.new()
	mesh.size = size
	var instance = MeshInstance3D.new()
	instance.mesh = mesh
	instance.position = pos
	var mat = StandardMaterial3D.new()
	mat.albedo_color = color
	mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	instance.material_override = mat
	world_root.add_child(instance)

func _add_camera():
	camera = Camera3D.new()
	camera.position = Vector3(8, 12, 14)
	add_child(camera)
	camera.look_at(Vector3.ZERO, Vector3.UP)

func _add_light():
	var light = DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-45, 45, 0)
	add_child(light)

func _add_ground():
	var mesh = PlaneMesh.new()
	mesh.size = Vector2(32, 32)
	var inst = MeshInstance3D.new()
	inst.mesh = mesh
	inst.position = Vector3(0, -0.01, 0)
	var mat = StandardMaterial3D.new()
	mat.albedo_color = Color(0.1, 0.1, 0.1, 1.0)
	inst.material_override = mat
	world_root.add_child(inst)

func _v3(value):
	if typeof(value) == TYPE_VECTOR3:
		return value
	return Vector3.ZERO

func _load_snapshot():
	if ClassDB.class_exists("WorldSnapshotProvider"):
		var provider = ClassDB.instantiate("WorldSnapshotProvider")
		if provider != null:
			var snap = provider.load_snapshot()
			if snap.size() > 0:
				return snap
			push_warning("WorldSnapshotProvider returned empty; falling back to JSON file.")
	else:
		push_warning("WorldSnapshotProvider not found; ensure the Harimu GDExtension is loaded.")

	var fallback_path = ProjectSettings.globalize_path("res://../../.harimu/world_snapshot.json")
	if FileAccess.file_exists(fallback_path):
		var text = FileAccess.get_file_as_string(fallback_path)
		var parsed = JSON.parse_string(text)
		if typeof(parsed) == TYPE_DICTIONARY:
			return parsed
	push_error("No snapshot available; run the simulation or infuse nodes first.")
	return {}

func _load_snapshots():
	var list = []
	var dir_path = ProjectSettings.globalize_path("res://../../.harimu/world_snapshots")
	var dir = DirAccess.open(dir_path)
	if dir:
		var files = []
		dir.list_dir_begin()
		while true:
			var f = dir.get_next()
			if f == "":
				break
			if dir.current_is_dir():
				continue
			if f.ends_with(".json"):
				files.append(f)
		files.sort()
		for f in files:
			var path = dir_path + "/" + f
			var text = FileAccess.get_file_as_string(path)
			var parsed = JSON.parse_string(text)
			if typeof(parsed) == TYPE_DICTIONARY:
				list.append(parsed)
	if list.size() > 0:
		return list

	var single = _load_snapshot()
	if single.size() > 0:
		return [single]
	return []

func _show_snapshot(index):
	if index < 0 or index >= snapshots.size():
		return
	current_index = index
	_render_snapshot(snapshots[current_index])
	label.text = "Tick %s | snapshot %d/%d | agents %d | ore %d | structures %d | space=play/pause, arrows=seek" % [
		snapshots[current_index].get("tick", 0),
		current_index + 1,
		snapshots.size(),
		snapshots[current_index].get("agents", []).size(),
		snapshots[current_index].get("ore_nodes", []).size(),
		snapshots[current_index].get("structures", []).size()
	]

func _advance(step):
	if snapshots.size() == 0:
		return
	var next = clamp(current_index + step, 0, snapshots.size() - 1)
	_show_snapshot(next)

func _zoom(direction):
	if camera == null:
		return
	var delta = direction * 1.0
	camera.translate_object_local(Vector3(0, 0, delta))
