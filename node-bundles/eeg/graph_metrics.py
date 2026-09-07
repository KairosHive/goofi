"""GraphMetrics — what a connectivity matrix looks like as a network.

Takes a square `[C, C]` matrix and answers the standard graph measures. Two are per node and keep
the channel names; the rest are one number for the whole graph.

A connectivity matrix is WEIGHTED and usually complete — every channel relates to every other one
a little — so the path length and the clustering of the raw matrix are the same for every patch.
`density` keeps the strongest edges and drops the rest, which is what makes these numbers move.
"""

import numpy as np
import goofi


class GraphMetrics(goofi.Node):
    """Clustering, path length, centrality and assortativity of a connectivity matrix.

    Threshold it with `density` first: a complete graph has nothing to say.
    """

    TAGS = ["analysis", "eeg", "connectivity"]
    INPUTS = {"matrix": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {
        "clustering": goofi.DataType.ARRAY,
        "path": goofi.DataType.ARRAY,
        "efficiency": goofi.DataType.ARRAY,
        "betweenness": goofi.DataType.ARRAY,
        "degree": goofi.DataType.ARRAY,
        "assortativity": goofi.DataType.ARRAY,
        "transitivity": goofi.DataType.ARRAY,
    }
    PARAMS = {
        "graph": {
            "density": goofi.FloatParam(
                0.2, 0.0, 1.0, doc="Fraction of the strongest edges to keep. 0 keeps the matrix as it is."
            ),
            "weighted": goofi.BoolParam(True, doc="Keep the surviving weights, or make every edge a 1."),
            "absolute": goofi.BoolParam(True, doc="Rank edges by magnitude, so an anticorrelation is a strong one."),
        }
    }

    def setup(self):
        import networkx as nx

        self.nx = nx

    def process(self, matrix):
        p = self.params.graph
        m = np.asarray(matrix.data, dtype=np.float64)
        if m.ndim != 2 or m.shape[0] != m.shape[1]:
            raise ValueError(f"needs a square matrix, got {list(m.shape)}")
        n = m.shape[0]
        if n < 3:
            raise ValueError(f"{n} nodes is not a graph")

        m = (m + m.T) / 2
        np.fill_diagonal(m, 0.0)
        strength = np.abs(m) if p.absolute else m

        if p.density > 0:
            off = ~np.eye(n, dtype=bool)
            keep = max(1, int(round(p.density * off.sum() / 2)))
            cut = np.sort(strength[np.triu(off, 1)])[::-1][keep - 1]
            m = np.where(strength >= cut, m, 0.0)
            strength = np.where(strength >= cut, strength, 0.0)

        graph = self.nx.from_numpy_array(strength if p.weighted else (strength > 0).astype(float))
        weight = "weight" if p.weighted else None

        clustering = float(self.nx.average_clustering(graph, weight=weight))
        transitivity = float(self.nx.transitivity(graph))
        # A thresholded graph is very often disconnected, and goofi 2 answered `None` for that —
        # which reached the wire as a shapeless frame. The largest component is the honest answer,
        # and `efficiency` is the one that is defined whether it is connected or not.
        parts = list(self.nx.connected_components(graph))
        biggest = graph.subgraph(max(parts, key=len)) if parts else graph
        path = float(self.nx.average_shortest_path_length(biggest)) if biggest.number_of_nodes() > 1 else 0.0
        efficiency = float(self.nx.global_efficiency(graph))
        betweenness = np.array(list(self.nx.betweenness_centrality(graph, weight=weight).values()))
        degree = strength.sum(axis=1) / max(n - 1, 1) if p.weighted else np.array(
            list(self.nx.degree_centrality(graph).values())
        )
        try:
            assortativity = float(self.nx.degree_assortativity_coefficient(graph))
        except (ValueError, ZeroDivisionError, FloatingPointError):
            # Undefined on a regular graph — every node the same degree leaves nothing to correlate.
            assortativity = 0.0
        if not np.isfinite(assortativity):
            assortativity = 0.0

        names = matrix.meta.get("channels", {}).get("dim0")
        per_node = {"channels": {"dim0": names}} if names else {}
        one = lambda v: (np.array([v], dtype=np.float32), {})
        return {
            "clustering": one(clustering),
            "path": one(path),
            "efficiency": one(efficiency),
            "transitivity": one(transitivity),
            "assortativity": one(assortativity),
            "betweenness": (betweenness.astype(np.float32), per_node),
            "degree": (degree.astype(np.float32), per_node),
        }
