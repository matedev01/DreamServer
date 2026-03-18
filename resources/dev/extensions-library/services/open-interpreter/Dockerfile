FROM python:3.12-slim@sha256:ccc7089399c8bb65dd1fb3ed6d55efa538a3f5e7fca3f5988ac3b5b87e593bf0

# Install dependencies
RUN pip install --no-cache-dir open-interpreter fastapi uvicorn pydantic

# Create non-root user
RUN useradd -m -s /bin/bash appuser

# Create app directory
WORKDIR /app

# Copy server script
COPY server.py /app/server.py

# Create data directory with correct ownership
RUN mkdir -p /app/data && chown -R appuser:appuser /app

# Switch to non-root user
USER appuser

# Expose port (matches server.py and compose.yaml)
EXPOSE 8080

# Default command: start FastAPI server
CMD ["python", "server.py"]
