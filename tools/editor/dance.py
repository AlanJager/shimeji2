"""Standing dance authored from the bundled shime's existing standing art."""


def standing_dance(project):
    parts = {part["id"]: part for part in project["parts"]}
    fields = ("x", "y", "rotation", "scaleX", "scaleY", "visible", "assetId")
    body = {field: parts["demo-body"][field] for field in fields}
    head = {field: parts["demo-head"][field] for field in fields}
    rotations = (0, -5, 0, 5, 0, -4, 0, 4, 0)
    head_rotations = (0, 6, 0, -6, 0, 4, 0, -4, 0)
    return {
        "id": "standing-dance", "name": "站着跳舞", "kind": "parts", "loop": True,
        "durationTicks": 40,
        "frames": [{"id": "dance-rest", "assetId": "shime1", "ticks": 40, "x": 0, "y": 0}],
        "tracks": [
            {"partId": "demo-body", "keys": [
                {**body, "tick": index * 5, "x": 64, "y": 126,
                 "scaleX": .94, "scaleY": .94, "rotation": rotation}
                for index, rotation in enumerate(rotations)]},
            {"partId": "demo-head", "keys": [
                {**head, "tick": index * 5, "rotation": rotation}
                for index, rotation in enumerate(head_rotations)]},
        ],
    }
