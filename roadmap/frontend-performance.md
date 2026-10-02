# Frontend performance: the owner's-display trace

## Remaining

- On the owner's display, take a 5 s Chrome Performance trace of
  `examples/harmonic-observatory.gfi`, fitted to the screen: idle, with the inspector open, and
  during a param drag. Read Commit / ProduceCanvasResource, FunctionCall, Layout and long tasks;
  note the DPR and whether a native graphics window is open. The headless trace shows no
  frame-rate drop; a GPU that the engine also uses is the one cost it cannot see.
