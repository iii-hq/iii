"""III SDK client instance for Motia framework."""

from .iii import get_instance

# Backward compatibility - bridge is now an alias for get_instance()
bridge = get_instance()
