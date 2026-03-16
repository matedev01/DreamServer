"""Tests for routers/workflows.py — workflow catalog, n8n integration, dependency checks."""

import json
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import aiohttp
import pytest

from routers.workflows import (
    load_workflow_catalog,
    get_n8n_workflows,
    check_workflow_dependencies,
    check_n8n_available,
)
from conftest import load_golden_fixture


# --- load_workflow_catalog ---


class TestLoadWorkflowCatalog:

    def test_returns_default_when_file_missing(self, monkeypatch):
        monkeypatch.setattr(
            "routers.workflows.WORKFLOW_CATALOG_FILE",
            Path("/nonexistent/catalog.json"),
        )
        result = load_workflow_catalog()
        assert result == {"workflows": [], "categories": {}}

    def test_loads_valid_catalog(self, tmp_path, monkeypatch):
        catalog = {
            "workflows": [
                {"id": "doc-qa", "name": "Document Q&A", "description": "RAG pipeline"}
            ],
            "categories": {"rag": "RAG Workflows"},
        }
        catalog_file = tmp_path / "catalog.json"
        catalog_file.write_text(json.dumps(catalog))
        monkeypatch.setattr("routers.workflows.WORKFLOW_CATALOG_FILE", catalog_file)

        result = load_workflow_catalog()
        assert len(result["workflows"]) == 1
        assert result["workflows"][0]["id"] == "doc-qa"
        assert result["categories"]["rag"] == "RAG Workflows"

    def test_returns_default_on_invalid_json(self, tmp_path, monkeypatch):
        catalog_file = tmp_path / "catalog.json"
        catalog_file.write_text("not valid json{{{")
        monkeypatch.setattr("routers.workflows.WORKFLOW_CATALOG_FILE", catalog_file)

        result = load_workflow_catalog()
        assert result == {"workflows": [], "categories": {}}


# --- get_n8n_workflows ---


class TestGetN8nWorkflows:

    @pytest.mark.asyncio
    async def test_returns_workflows_on_success(self, monkeypatch):
        fixture_data = load_golden_fixture("n8n_workflows.json")

        response = AsyncMock()
        response.status = 200
        response.json = AsyncMock(return_value=fixture_data)

        ctx = AsyncMock()
        ctx.__aenter__ = AsyncMock(return_value=response)
        ctx.__aexit__ = AsyncMock(return_value=False)

        session = AsyncMock()
        session.get = MagicMock(return_value=ctx)
        session.__aenter__ = AsyncMock(return_value=session)
        session.__aexit__ = AsyncMock(return_value=False)

        monkeypatch.setattr("routers.workflows.aiohttp.ClientSession", lambda **kw: session)

        result = await get_n8n_workflows()
        assert len(result) == 2
        assert result[0]["name"] == "Document Q&A"

    @pytest.mark.asyncio
    async def test_returns_empty_on_failure(self, monkeypatch):
        session = AsyncMock()
        session.get = MagicMock(side_effect=aiohttp.ClientError("connection refused"))
        session.__aenter__ = AsyncMock(return_value=session)
        session.__aexit__ = AsyncMock(return_value=False)

        monkeypatch.setattr("routers.workflows.aiohttp.ClientSession", lambda **kw: session)

        result = await get_n8n_workflows()
        assert result == []

    @pytest.mark.asyncio
    async def test_includes_api_key_header(self, monkeypatch):
        monkeypatch.setattr("routers.workflows.N8N_API_KEY", "test-api-key")

        captured_headers = {}

        response = AsyncMock()
        response.status = 200
        response.json = AsyncMock(return_value={"data": []})

        ctx = AsyncMock()
        ctx.__aenter__ = AsyncMock(return_value=response)
        ctx.__aexit__ = AsyncMock(return_value=False)

        session = AsyncMock()

        def capture_get(url, headers=None):
            if headers:
                captured_headers.update(headers)
            return ctx

        session.get = capture_get
        session.__aenter__ = AsyncMock(return_value=session)
        session.__aexit__ = AsyncMock(return_value=False)

        monkeypatch.setattr("routers.workflows.aiohttp.ClientSession", lambda **kw: session)

        await get_n8n_workflows()
        assert captured_headers.get("X-N8N-API-KEY") == "test-api-key"


# --- check_workflow_dependencies ---


class TestCheckWorkflowDependencies:

    @pytest.mark.asyncio
    async def test_healthy_service_returns_true(self, monkeypatch):
        from models import ServiceStatus
        fake_services = {
            "llama-server": {"name": "LLM", "port": 8080, "external_port": 8080, "health": "/health", "host": "localhost"},
        }
        monkeypatch.setattr("routers.workflows.SERVICES", fake_services)

        async def fake_health(sid, cfg):
            return ServiceStatus(id=sid, name=cfg["name"], port=cfg["port"],
                                 external_port=cfg["external_port"], status="healthy")

        monkeypatch.setattr("helpers.check_service_health", fake_health)

        result = await check_workflow_dependencies(["llama-server"])
        assert result["llama-server"] is True

    @pytest.mark.asyncio
    async def test_unhealthy_service_returns_false(self, monkeypatch):
        from models import ServiceStatus
        fake_services = {
            "llama-server": {"name": "LLM", "port": 8080, "external_port": 8080, "health": "/health", "host": "localhost"},
        }
        monkeypatch.setattr("routers.workflows.SERVICES", fake_services)

        async def fake_health(sid, cfg):
            return ServiceStatus(id=sid, name=cfg["name"], port=cfg["port"],
                                 external_port=cfg["external_port"], status="down")

        monkeypatch.setattr("helpers.check_service_health", fake_health)

        result = await check_workflow_dependencies(["llama-server"])
        assert result["llama-server"] is False

    @pytest.mark.asyncio
    async def test_alias_resolution(self, monkeypatch):
        """'ollama' should resolve to 'llama-server'."""
        from models import ServiceStatus
        fake_services = {
            "llama-server": {"name": "LLM", "port": 8080, "external_port": 8080, "health": "/health", "host": "localhost"},
        }
        monkeypatch.setattr("routers.workflows.SERVICES", fake_services)

        async def fake_health(sid, cfg):
            return ServiceStatus(id=sid, name=cfg["name"], port=cfg["port"],
                                 external_port=cfg["external_port"], status="healthy")

        monkeypatch.setattr("helpers.check_service_health", fake_health)

        result = await check_workflow_dependencies(["ollama"])
        assert result["ollama"] is True


# --- check_n8n_available ---


class TestCheckN8nAvailable:

    @pytest.mark.asyncio
    async def test_true_on_healthy_response(self, monkeypatch):
        response = AsyncMock()
        response.status = 200

        ctx = AsyncMock()
        ctx.__aenter__ = AsyncMock(return_value=response)
        ctx.__aexit__ = AsyncMock(return_value=False)

        session = AsyncMock()
        session.get = MagicMock(return_value=ctx)
        session.__aenter__ = AsyncMock(return_value=session)
        session.__aexit__ = AsyncMock(return_value=False)

        monkeypatch.setattr("routers.workflows.aiohttp.ClientSession", lambda **kw: session)

        assert await check_n8n_available() is True

    @pytest.mark.asyncio
    async def test_false_on_error(self, monkeypatch):
        session = AsyncMock()
        session.get = MagicMock(side_effect=aiohttp.ClientError("unreachable"))
        session.__aenter__ = AsyncMock(return_value=session)
        session.__aexit__ = AsyncMock(return_value=False)

        monkeypatch.setattr("routers.workflows.aiohttp.ClientSession", lambda **kw: session)

        assert await check_n8n_available() is False


# --- /api/workflows endpoint ---


class TestApiWorkflowsEndpoint:

    def test_returns_enriched_workflows(self, test_client, monkeypatch):
        catalog = {
            "workflows": [
                {
                    "id": "doc-qa",
                    "name": "Document Q&A",
                    "description": "RAG pipeline",
                    "icon": "FileText",
                    "category": "rag",
                    "dependencies": [],
                }
            ],
            "categories": {"rag": "RAG"},
        }
        monkeypatch.setattr("routers.workflows.load_workflow_catalog", lambda: catalog)
        monkeypatch.setattr("routers.workflows.get_n8n_workflows", AsyncMock(return_value=[]))
        monkeypatch.setattr("routers.workflows.check_n8n_available", AsyncMock(return_value=False))
        monkeypatch.setattr("routers.workflows.check_workflow_dependencies", AsyncMock(return_value={}))

        resp = test_client.get("/api/workflows", headers=test_client.auth_headers)
        assert resp.status_code == 200
        data = resp.json()
        assert "workflows" in data
        assert len(data["workflows"]) == 1
        assert data["workflows"][0]["id"] == "doc-qa"
        assert data["workflows"][0]["status"] == "available"


# --- POST /api/workflows/{id}/enable ---


class TestEnableWorkflow:

    def test_rejects_invalid_id_format(self, test_client):
        resp = test_client.post(
            "/api/workflows/../../etc/passwd/enable",
            headers=test_client.auth_headers,
        )
        assert resp.status_code in (400, 404, 422)

    def test_404_when_not_in_catalog(self, test_client, monkeypatch):
        monkeypatch.setattr(
            "routers.workflows.load_workflow_catalog",
            lambda: {"workflows": [], "categories": {}},
        )

        resp = test_client.post(
            "/api/workflows/nonexistent-wf/enable",
            headers=test_client.auth_headers,
        )
        assert resp.status_code == 404


# --- DELETE /api/workflows/{id} ---


class TestDisableWorkflow:

    def test_404_when_not_in_catalog(self, test_client, monkeypatch):
        monkeypatch.setattr(
            "routers.workflows.load_workflow_catalog",
            lambda: {"workflows": [], "categories": {}},
        )
        monkeypatch.setattr("routers.workflows.get_n8n_workflows", AsyncMock(return_value=[]))

        resp = test_client.delete(
            "/api/workflows/nonexistent-wf",
            headers=test_client.auth_headers,
        )
        assert resp.status_code == 404
