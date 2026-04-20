"""Core protocol definitions for the I2I messaging system."""

from __future__ import annotations

import json
import time
from dataclasses import asdict, dataclass, field
from typing import Any, Optional


@dataclass
class I2IMessage:
    """A single I2I inter-agent message.

    Attributes:
        sender: Identifier of the sending agent.
        recipient: Identifier of the intended receiving agent.
        msg_type: Logical message type (e.g., ``QUERY``, ``RESPONSE``).
        content: Payload of the message.
        priority: Priority level -- ``P0``, ``P1``, or ``P2`` (lowest).
        timestamp: Unix epoch timestamp (float) when the message was created.
        metadata: Additional key/value context attached to the message.
    """

    sender: str
    recipient: str
    msg_type: str
    content: str
    priority: str = "P2"
    timestamp: float = field(default_factory=time.time)
    metadata: dict[str, Any] = field(default_factory=dict)


class I2IProtocol:
    """Message formatting, parsing, serialization, and validation utilities."""

    _PRIORITY_RANK = {"P0": 0, "P1": 1, "P2": 2}

    @staticmethod
    def format_message(
        sender: str,
        recipient: str,
        msg_type: str,
        content: str,
        priority: str = "P2",
    ) -> I2IMessage:
        """Create a fully populated :class:`I2IMessage`.

        Args:
            sender: Sending agent identifier.
            recipient: Target agent identifier.
            msg_type: Message type discriminator.
            content: Message body.
            priority: ``P0`` / ``P1`` / ``P2`` (default ``P2``).

        Returns:
            A new :class:`I2IMessage` instance with the current timestamp.
        """
        return I2IMessage(
            sender=sender,
            recipient=recipient,
            msg_type=msg_type,
            content=content,
            priority=priority,
        )

    @classmethod
    def parse_message(cls, raw: str) -> I2IMessage:
        """Parse a human-readable ``[I2I:TYPE] sender -> recipient — content`` string.

        Args:
            raw: Input string in the human-readable format.

        Returns:
            The parsed :class:`I2IMessage`.

        Raises:
            ValueError: If *raw* does not conform to the expected pattern.
        """
        # Example: [I2I:QUERY] alice -> bob — hello world
        if not raw.startswith("[I2I:"):
            raise ValueError("Message must start with '[I2I:'")

        type_end = raw.find("]", 5)
        if type_end == -1:
            raise ValueError("Missing closing ']' for message type")

        msg_type = raw[5:type_end].strip()
        remainder = raw[type_end + 1 :].strip()

        arrow = remainder.find("->")
        if arrow == -1:
            raise ValueError("Missing '->' separator")

        sender = remainder[:arrow].strip()
        after_arrow = remainder[arrow + 2 :].strip()

        em_dash = after_arrow.find("—")
        if em_dash == -1:
            # Fallback to regular hyphen/dash for convenience
            em_dash = after_arrow.find("- ")
            if em_dash == -1:
                raise ValueError("Missing '—' content separator")
            recipient = after_arrow[:em_dash].strip()
            content = after_arrow[em_dash + 1 :].strip()
        else:
            recipient = after_arrow[:em_dash].strip()
            content = after_arrow[em_dash + 1 :].strip()

        return I2IMessage(
            sender=sender,
            recipient=recipient,
            msg_type=msg_type,
            content=content,
        )

    @staticmethod
    def serialize(message: I2IMessage) -> str:
        """Convert an :class:`I2IMessage` to a wire-format JSON string.

        Args:
            message: Message to serialize.

        Returns:
            JSON-encoded string suitable for network transmission.
        """
        return json.dumps(asdict(message), separators=(",", ":"))

    @classmethod
    def deserialize(cls, raw: str) -> I2IMessage:
        """Reconstruct an :class:`I2IMessage` from a JSON wire-format string.

        Args:
            raw: JSON string previously produced by :meth:`serialize`.

        Returns:
            The reconstructed :class:`I2IMessage`.

        Raises:
            ValueError: If *raw* is not valid JSON or missing required keys.
        """
        try:
            payload: dict[str, Any] = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise ValueError(f"Invalid JSON: {exc}") from exc

        required = {"sender", "recipient", "msg_type", "content"}
        missing = required - set(payload.keys())
        if missing:
            raise ValueError(f"Missing required fields: {missing}")

        return I2IMessage(
            sender=payload["sender"],
            recipient=payload["recipient"],
            msg_type=payload["msg_type"],
            content=payload["content"],
            priority=payload.get("priority", "P2"),
            timestamp=payload.get("timestamp", time.time()),
            metadata=payload.get("metadata", {}),
        )

    @classmethod
    def validate(cls, message: I2IMessage) -> bool:
        """Check whether *message* has all required fields populated.

        A field is considered populated when it is a non-empty string.

        Args:
            message: Message to inspect.

        Returns:
            ``True`` if the message passes validation, else ``False``.
        """
        return all(
            [
                isinstance(message.sender, str) and message.sender.strip() != "",
                isinstance(message.recipient, str) and message.recipient.strip() != "",
                isinstance(message.msg_type, str) and message.msg_type.strip() != "",
                isinstance(message.content, str) and message.content.strip() != "",
            ]
        )

    @classmethod
    def priority_gate(cls, message: I2IMessage, min_priority: str = "P2") -> bool:
        """Block messages whose priority is below the supplied threshold.

        Lower numeric rank == higher priority (``P0`` > ``P1`` > ``P2``).

        Args:
            message: Message to evaluate.
            min_priority: Minimum allowed priority (default ``P2``).

        Returns:
            ``True`` if *message* meets or exceeds the threshold.
        """
        msg_rank = cls._PRIORITY_RANK.get(message.priority, 99)
        threshold_rank = cls._PRIORITY_RANK.get(min_priority, 99)
        return msg_rank <= threshold_rank


class TrustRouter:
    """Routes messages to the most trusted agent among a set of candidates.

    Trust levels are floats in the inclusive range ``0.0`` to ``1.0``.
    """

    def __init__(self) -> None:
        """Initialise an empty trust table."""
        self._trust: dict[str, float] = {}

    def add_trust(self, agent_id: str, trust_level: float) -> None:
        """Register or update an agent's trust level.

        Args:
            agent_id: Unique agent identifier.
            trust_level: Trust score between ``0.0`` and ``1.0`` inclusive.

        Raises:
            ValueError: If *trust_level* is outside the ``[0.0, 1.0]`` range.
        """
        if not 0.0 <= trust_level <= 1.0:
            raise ValueError("trust_level must be between 0.0 and 1.0")
        self._trust[agent_id] = trust_level

    def get_trust(self, agent_id: str) -> float:
        """Return the stored trust level for *agent_id*.

        Args:
            agent_id: Agent identifier.

        Returns:
            The trust level if known, otherwise ``0.0``.
        """
        return self._trust.get(agent_id, 0.0)

    def route(self, message: I2IMessage, available_agents: list[str]) -> Optional[str]:
        """Pick the most-trusted agent from *available_agents*.

        Args:
            message: The message being routed (available for future extensibility).
            available_agents: Candidate agent identifiers.

        Returns:
            The identifier of the most trusted available agent, or ``None`` if
            no candidates are supplied or none are trusted.
        """
        if not available_agents:
            return None

        best_agent: Optional[str] = None
        best_score = -1.0

        for agent in available_agents:
            score = self.get_trust(agent)
            if score > best_score:
                best_score = score
                best_agent = agent

        # If the best agent has 0.0 trust they are effectively untrusted.
        if best_score <= 0.0:
            return None

        return best_agent

    def is_trusted(self, agent_id: str, min_trust: float = 0.3) -> bool:
        """Check whether *agent_id* meets the minimum trust threshold.

        Args:
            agent_id: Agent identifier.
            min_trust: Minimum acceptable trust score (default ``0.3``).

        Returns:
            ``True`` if the agent's stored trust is >= *min_trust*.
        """
        return self.get_trust(agent_id) >= min_trust

    def stats(self) -> dict[str, Any]:
        """Return aggregate statistics for the trust table.

        Returns:
            Dictionary with keys ``count``, ``average_trust``, ``max_trust``,
            and ``min_trust``.  If the table is empty all numeric values are
            ``0.0``.
        """
        if not self._trust:
            return {
                "count": 0,
                "average_trust": 0.0,
                "max_trust": 0.0,
                "min_trust": 0.0,
            }

        values = list(self._trust.values())
        return {
            "count": len(values),
            "average_trust": sum(values) / len(values),
            "max_trust": max(values),
            "min_trust": min(values),
        }
