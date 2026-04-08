# Phase 2: Python SDK Generation (0% → 100%)

## Target: Complete Python SDK with httpx, Pydantic, async/sync support

## Week 1: Python Foundation

### 1.1 Python Client Template with httpx
```python
# templates/python/client.py.template
import asyncio
from typing import Optional, Dict, Any, Union
import httpx
from pydantic import BaseModel, ValidationError
from .models import *
from .exceptions import *

class {{clientName}}Config(BaseModel):
    base_url: str
    api_key: Optional[str] = None
    bearer_token: Optional[str] = None
    timeout: float = 30.0
    retries: int = 3

class {{clientName}}:
    """{{apiDescription}}"""
    
    def __init__(self, config: {{clientName}}Config):
        self.config = config
        self._client = httpx.Client(
            base_url=config.base_url,
            timeout=config.timeout,
            headers=self._build_headers()
        )
        self._async_client = httpx.AsyncClient(
            base_url=config.base_url,
            timeout=config.timeout,
            headers=self._build_headers()
        )

    def _build_headers(self) -> Dict[str, str]:
        headers = {
            "Content-Type": "application/json",
            "User-Agent": f"{{clientName}}/{{version}} (Python)"
        }
        
        if self.config.api_key:
            headers["X-API-Key"] = self.config.api_key
        
        if self.config.bearer_token:
            headers["Authorization"] = f"Bearer {self.config.bearer_token}"
            
        return headers

    {{#operations}}
    def {{operation_name}}(self{{#parameters}}, {{name}}: {{type}}{{#optional}} = None{{/optional}}{{/parameters}}) -> {{return_type}}:
        """{{description}}"""
        response = self._client.{{http_method}}(
            "{{path}}"{{#has_body}},
            json={{body_param}}.dict() if isinstance({{body_param}}, BaseModel) else {{body_param}}{{/has_body}}{{#has_params}},
            params={{params_dict}}{{/has_params}}
        )
        self._handle_response(response)
        return {{return_type}}.parse_obj(response.json())

    async def {{operation_name}}_async(self{{#parameters}}, {{name}}: {{type}}{{#optional}} = None{{/optional}}{{/parameters}}) -> {{return_type}}:
        """{{description}} (async version)"""
        response = await self._async_client.{{http_method}}(
            "{{path}}"{{#has_body}},
            json={{body_param}}.dict() if isinstance({{body_param}}, BaseModel) else {{body_param}}{{/has_body}}{{#has_params}},
            params={{params_dict}}{{/has_params}}
        )
        self._handle_response(response)
        return {{return_type}}.parse_obj(response.json())
    {{/operations}}

    def _handle_response(self, response: httpx.Response) -> None:
        if response.status_code == 401:
            raise AuthenticationError("Authentication failed")
        elif response.status_code == 400:
            raise ValidationError("Validation failed", response.json())
        elif response.status_code >= 400:
            raise {{clientName}}Error(f"HTTP {response.status_code}: {response.text}")

    def close(self):
        """Close the HTTP client"""
        self._client.close()
        
    async def aclose(self):
        """Close the async HTTP client"""
        await self._async_client.aclose()

    def __enter__(self):
        return self
        
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()
        
    async def __aenter__(self):
        return self
        
    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await self.aclose()
