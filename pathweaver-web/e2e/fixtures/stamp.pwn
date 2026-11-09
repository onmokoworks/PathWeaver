{
  "kind": "pathweaver/look",
  "version": 1,
  "name": "スタンプ",
  "graph": {
    "enabled": true,
    "name": "スタンプ",
    "nodes": [
      {
        "a": 10,
        "amount": 3,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "bypass": false,
        "color": [
          224,
          224,
          224
        ],
        "detail": 6,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 1,
        "inner": false,
        "inputs": [],
        "keep_source": true,
        "kind": "Input",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          30,
          90
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      },
      {
        "a": 10,
        "amount": 3,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "bypass": false,
        "color": [
          224,
          224,
          224
        ],
        "detail": 6,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 2,
        "inner": false,
        "inputs": [
          17
        ],
        "keep_source": true,
        "kind": "Output",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          2150,
          90
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      },
      {
        "a": 10,
        "amount": 3,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "bypass": false,
        "color": [
          221,
          221,
          221
        ],
        "detail": 6,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 10,
        "inner": false,
        "inputs": [
          1
        ],
        "keep_source": true,
        "kind": "Fill",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          310,
          90
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      },
      {
        "a": 10,
        "amount": 26,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "bypass": false,
        "color": [
          0,
          0,
          0
        ],
        "detail": 6,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 11,
        "inner": true,
        "inputs": [
          10
        ],
        "keep_source": true,
        "kind": "Glow",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          540,
          130
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      },
      {
        "a": 10,
        "amount": 0.5,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "by_darkness": true,
        "bypass": false,
        "color": [
          224,
          224,
          224
        ],
        "detail": 1,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 12,
        "inner": false,
        "inputs": [
          11
        ],
        "keep_source": true,
        "kind": "Noise",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          770,
          90
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 3,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      },
      {
        "a": 10,
        "amount": 3,
        "b": 1,
        "bindings": [],
        "blur": 0.6,
        "both": false,
        "bypass": false,
        "color": [
          224,
          224,
          224
        ],
        "detail": 6,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 13,
        "inner": false,
        "inputs": [
          12
        ],
        "keep_source": true,
        "kind": "RasterBlur",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          1000,
          130
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      },
      {
        "a": 10,
        "amount": 0.42,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "bypass": false,
        "color": [
          224,
          224,
          224
        ],
        "detail": 6,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 14,
        "inner": false,
        "inputs": [
          13
        ],
        "keep_source": true,
        "kind": "Threshold",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          1230,
          90
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      },
      {
        "a": 10,
        "amount": 0.5,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "bypass": false,
        "color": [
          224,
          224,
          224
        ],
        "detail": 0.5,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 15,
        "inner": false,
        "inputs": [
          14
        ],
        "keep_source": true,
        "kind": "Trace",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          1460,
          130
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 1,
        "translation": [
          0,
          0
        ],
        "value": 2
      },
      {
        "a": 10,
        "amount": 1,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "bypass": false,
        "color": [
          224,
          224,
          224
        ],
        "detail": 30,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 16,
        "inner": false,
        "inputs": [
          15
        ],
        "keep_source": true,
        "kind": "Rough",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          1690,
          90
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      },
      {
        "a": 10,
        "amount": 3,
        "b": 1,
        "bindings": [],
        "blur": 0,
        "both": false,
        "bypass": false,
        "color": [
          200,
          40,
          40
        ],
        "detail": 6,
        "effect": {
          "amount": 10,
          "angle": 0,
          "detail": 2,
          "grain": "Dots",
          "join": "Round",
          "position": "Outer",
          "seed": 1,
          "spread": 12
        },
        "id": 17,
        "inner": false,
        "inputs": [
          16
        ],
        "keep_source": true,
        "kind": "Fill",
        "label": "",
        "math": "Add",
        "measure_height": false,
        "opacity": 1,
        "operation": "Union",
        "position": [
          1920,
          130
        ],
        "rotation": 0,
        "scale": 1,
        "seed": 1,
        "smooth": true,
        "spread": 0,
        "translation": [
          0,
          0
        ],
        "value": 10
      }
    ],
    "output": 2,
    "shared": null
  }
}